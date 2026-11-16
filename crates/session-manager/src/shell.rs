//! 轻量 Shell 解析器。
//!
//! 只做命令安全判断需要的最小解析：识别引号、转义、命令分隔符、管道、
//! 重定向、命令替换与后台执行。不追求完整 Shell AST。

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedCommand {
    /// 按 `;`、`&&`、`||`、`|`、换行切分后的命令段，每段已按空白切词。
    pub segments: Vec<Vec<String>>,
    /// 是否出现输出/输入/文件描述符重定向或 heredoc。
    pub has_redirection: bool,
    /// 是否出现 `$(...)` 或反引号命令替换。
    pub has_command_substitution: bool,
    /// 是否出现单个 `&` 后台执行。
    pub has_background: bool,
}

fn flush_token(current: &mut String, started: &mut bool, tokens: &mut Vec<String>) {
    if *started || !current.is_empty() {
        tokens.push(std::mem::take(current));
        *started = false;
    }
}

fn flush_segment(tokens: &mut Vec<String>, segments: &mut Vec<Vec<String>>) {
    if !tokens.is_empty() {
        segments.push(std::mem::take(tokens));
    }
}

/// 解析命令。引号或转义未闭合时返回错误。
///
/// 重定向、命令替换、后台执行只做标记，不解引用；调用方按权限决定是否拒绝。
pub fn parse(command: &str) -> Result<ParsedCommand, String> {
    let mut parsed = ParsedCommand::default();
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            started = true;
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                escaped = true;
                started = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                started = true;
            }
            '`' if !in_single => {
                parsed.has_command_substitution = true;
                current.push(ch);
                started = true;
            }
            '$' if !in_single => {
                if chars.peek() == Some(&'(') {
                    parsed.has_command_substitution = true;
                }
                current.push(ch);
                started = true;
            }
            '>' | '<' if !in_single && !in_double => {
                parsed.has_redirection = true;
                flush_token(&mut current, &mut started, &mut tokens);
            }
            '&' if !in_single && !in_double => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    flush_token(&mut current, &mut started, &mut tokens);
                    flush_segment(&mut tokens, &mut parsed.segments);
                } else if chars.peek() == Some(&'>') {
                    // `&>` / `&>>` 重定向
                    chars.next();
                    parsed.has_redirection = true;
                    flush_token(&mut current, &mut started, &mut tokens);
                } else {
                    parsed.has_background = true;
                    flush_token(&mut current, &mut started, &mut tokens);
                    flush_segment(&mut tokens, &mut parsed.segments);
                }
            }
            '|' if !in_single && !in_double => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                flush_token(&mut current, &mut started, &mut tokens);
                flush_segment(&mut tokens, &mut parsed.segments);
            }
            ';' | '\n' if !in_single && !in_double => {
                flush_token(&mut current, &mut started, &mut tokens);
                flush_segment(&mut tokens, &mut parsed.segments);
            }
            ch if ch.is_whitespace() && !in_single && !in_double => {
                flush_token(&mut current, &mut started, &mut tokens);
            }
            ch => {
                current.push(ch);
                started = true;
            }
        }
    }

    if escaped || in_single || in_double {
        return Err("命令包含未闭合的引号或转义".into());
    }
    flush_token(&mut current, &mut started, &mut tokens);
    flush_segment(&mut tokens, &mut parsed.segments);

    if parsed.segments.is_empty() {
        return Err("命令不能为空".into());
    }
    Ok(parsed)
}

/// 提取程序名：去路径、去首尾引号、转小写。
pub fn program_name(raw: &str) -> String {
    let raw = raw.trim_matches(|ch| ch == '"' || ch == '\'');
    raw.rsplit('/').next().unwrap_or(raw).to_ascii_lowercase()
}

/// 跳过命令前的环境变量赋值（`FOO=bar cmd`）。
pub fn skip_assignments(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() && is_assignment(&tokens[index]) {
        index += 1;
    }
    &tokens[index..]
}

fn is_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_pipes_and_segments() {
        let parsed = parse("ps aux | grep nginx; df -h").expect("解析成功");
        assert_eq!(
            parsed.segments,
            vec![
                vec!["ps".to_string(), "aux".to_string()],
                vec!["grep".to_string(), "nginx".to_string()],
                vec!["df".to_string(), "-h".to_string()],
            ]
        );
        assert!(!parsed.has_redirection);
        assert!(!parsed.has_command_substitution);
    }

    #[test]
    fn marks_redirection_and_substitution() {
        let redirection = parse("echo hi > /tmp/x").expect("解析成功");
        assert!(redirection.has_redirection);
        let substitution = parse("echo $(whoami)").expect("解析成功");
        assert!(substitution.has_command_substitution);
        let backtick = parse("echo `whoami`").expect("解析成功");
        assert!(backtick.has_command_substitution);
    }

    #[test]
    fn handles_quoted_operators_as_literals() {
        let parsed = parse("grep 'a > b' file").expect("解析成功");
        assert!(!parsed.has_redirection);
        assert_eq!(
            parsed.segments[0],
            vec!["grep".to_string(), "a > b".to_string(), "file".to_string()]
        );
    }

    #[test]
    fn rejects_unclosed_quotes() {
        assert!(parse("echo 'oops").is_err());
        assert!(parse("echo \"oops").is_err());
    }

    #[test]
    fn normalizes_quoted_program_names() {
        assert_eq!(program_name("/usr/bin/ls"), "ls");
        assert_eq!(program_name("'sudo'"), "sudo");
        let parsed = parse("s\"u\"do ls").expect("解析成功");
        assert_eq!(program_name(&parsed.segments[0][0]), "sudo");
    }
}
