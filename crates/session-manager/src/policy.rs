//! 命令安全策略。
//!
//! 安全边界说明：字符串规则无法为通用 Shell 提供可靠的只读保证。
//! ReadOnly 采用「允许清单 + 未知拒绝」，WriteAccess 采用「默认允许 + 灾难/提权拦截」。
//! 两者都只是纵深防御，最终边界是连接账号的系统权限。

use std::str::FromStr;

use umt_core::TokenPermission;

use crate::shell::{self, ParsedCommand};

/// 全局执行模式。保留该类型用于系统设置兼容，不再参与 MCP 权限判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    ReadOnly,
    #[default]
    Restricted,
    FullAccess,
}

impl FromStr for ExecutionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "ReadOnly" => Ok(Self::ReadOnly),
            "Restricted" => Ok(Self::Restricted),
            "FullAccess" => Ok(Self::FullAccess),
            _ => Err(format!(
                "未知执行模式：{value}（可选 ReadOnly / Restricted / FullAccess）"
            )),
        }
    }
}

/// 单条命令的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}

/// 按 Token 权限评估命令。
pub struct CommandGuard;

impl CommandGuard {
    pub fn evaluate(permission: TokenPermission, command: &str) -> PolicyDecision {
        if command.trim().is_empty() {
            return deny("命令不能为空");
        }
        let parsed = match shell::parse(command) {
            Ok(parsed) => parsed,
            Err(reason) => return deny(reason),
        };

        if let Some(program) = find_privilege_escalation(&parsed) {
            return deny(format!("禁止执行提权命令：{program}"));
        }

        match permission {
            TokenPermission::ReadOnly => evaluate_read_only(&parsed),
            TokenPermission::WriteAccess => evaluate_write_access(&parsed, command),
        }
    }
}

fn deny(reason: impl Into<String>) -> PolicyDecision {
    PolicyDecision::Deny {
        reason: reason.into(),
    }
}

fn evaluate_read_only(parsed: &ParsedCommand) -> PolicyDecision {
    if parsed.has_redirection {
        return deny("ReadOnly 不允许输出/输入重定向或 heredoc");
    }
    if parsed.has_command_substitution {
        return deny("ReadOnly 不允许命令替换（$() 或反引号）");
    }
    if parsed.has_background {
        return deny("ReadOnly 不允许后台执行");
    }

    for segment in &parsed.segments {
        let tokens = shell::skip_assignments(segment);
        let Some(program_raw) = tokens.first() else {
            continue;
        };
        let program = shell::program_name(program_raw);
        let args = &tokens[1..];
        if !read_only_allowed(&program, args) {
            return deny(format!("ReadOnly 不允许执行该命令或参数：{program}"));
        }
    }
    PolicyDecision::Allow
}

/// ReadOnly 允许清单。未知命令一律拒绝。
fn read_only_allowed(program: &str, args: &[String]) -> bool {
    match program {
        // 文件与文本
        "ls" | "cat" | "head" | "tail" | "grep" | "stat" | "wc" | "du" | "df" | "readlink"
        | "realpath" | "dirname" | "basename" | "which" | "type" | "cmp" | "jq" | "base64"
        | "md5sum" | "sha256sum" | "seq" | "expr" => true,
        "echo" | "printf" => true,
        // `file -C` 会编译 magic 文件；只允许普通查询。
        "file" => !args.iter().any(|arg| arg == "-C" || arg == "--compile"),
        // `tree -o` 会写文件。
        "tree" => !args.iter().any(|arg| arg == "-o"),
        // `diff -o/--output` 会写补丁文件。
        "diff" => !args
            .iter()
            .any(|arg| arg == "-o" || arg == "--output" || arg.starts_with("--output=")),
        "rg" => !args
            .iter()
            .any(|arg| arg == "--pre" || arg.starts_with("--pre=")),
        "find" => !args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "-delete"
                    | "-exec"
                    | "-execdir"
                    | "-ok"
                    | "-okdir"
                    | "-fprint"
                    | "-fprint0"
                    | "-fprintf"
                    | "-fls"
            )
        }),

        // 系统与进程
        "ps" | "free" | "uname" | "uptime" | "whoami" | "id" | "hostname" | "date" | "w"
        | "who" | "lsof" | "findmnt" | "lsmod" | "modinfo" | "lsblk" | "lscpu" | "lsmem"
        | "lsusb" | "lspci" | "nproc" | "arch" | "getent" | "getfacl" | "getcap" | "last"
        | "lastlog" | "printenv" => true,
        "top" => args.first().is_some_and(|arg| arg.starts_with("-b")),
        "ulimit" => args.is_empty() || args.first().is_some_and(|arg| arg == "-a"),
        "dmesg" => !args.iter().any(|arg| {
            matches!(arg.as_str(), "--clear" | "--read-clear")
                || (arg.starts_with('-')
                    && !arg.starts_with("--")
                    && (arg.contains('c') || arg.contains('C')))
        }),
        "env" => args.is_empty(),

        // 网络只读
        "ss" | "netstat" | "ping" | "traceroute" | "dig" | "nslookup" | "host" => true,
        "ip" => {
            let read_only_action = |arg: &str| matches!(arg, "show" | "list" | "get" | "s" | "ls");
            match args.first().map(String::as_str) {
                Some("addr" | "a" | "route" | "r" | "link" | "l" | "neigh" | "n") => {
                    args.get(1).is_none_or(|arg| read_only_action(arg))
                }
                Some("show" | "s") => true,
                _ => false,
            }
        }
        "resolvectl" => args
            .first()
            .is_none_or(|arg| matches!(arg.as_str(), "status" | "query" | "statistics")),
        "nmcli" => {
            (args.first().is_some_and(|arg| arg == "device")
                && args.get(1).is_some_and(|arg| arg == "status"))
                || (args.first().is_some_and(|arg| arg == "connection")
                    && args.get(1).is_some_and(|arg| arg == "show"))
        }
        "iptables" | "ip6tables" => {
            let listing = args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "-L" | "-S" | "--list" | "--list-rules"));
            let mutation = args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-F" | "-X"
                        | "-A"
                        | "-I"
                        | "-D"
                        | "-R"
                        | "-N"
                        | "-P"
                        | "-E"
                        | "-Z"
                        | "--flush"
                        | "--delete-chain"
                        | "--append"
                        | "--insert"
                        | "--delete"
                        | "--replace"
                        | "--new-chain"
                        | "--policy"
                        | "--rename-chain"
                        | "--zero"
                )
            });
            listing && !mutation
        }
        "nft" => args.first().is_some_and(|arg| arg == "list"),
        "ufw" => args.first().is_some_and(|arg| arg == "status"),

        // 服务与日志
        "systemctl" => args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "status"
                    | "list-units"
                    | "list-unit-files"
                    | "list-timers"
                    | "list-sockets"
                    | "show"
                    | "cat"
                    | "is-active"
                    | "is-enabled"
                    | "is-failed"
            )
        }),
        "journalctl" => !args
            .iter()
            .any(|arg| arg.starts_with("--vacuum-") || arg == "--rotate" || arg == "--flush"),
        "crontab" => {
            args.first().is_some_and(|arg| arg == "-l")
                && !args.iter().any(|arg| matches!(arg.as_str(), "-r" | "-e"))
        }

        // 主机状态
        "timedatectl" | "hostnamectl" | "localectl" => args
            .first()
            .is_none_or(|arg| arg == "status" || arg == "show"),
        "loginctl" => args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "list-sessions" | "show-session" | "session-status"
            )
        }),

        // 包管理只读
        "apt" | "apt-get" => {
            args.first().is_some_and(|arg| arg == "list")
                && args.get(1).is_some_and(|arg| arg == "--installed")
        }
        "apt-cache" => args.first().is_some_and(|arg| arg == "policy"),
        "dpkg" => {
            let query = args.first().is_some_and(|arg| {
                matches!(arg.as_str(), "-l" | "-s" | "-L" | "--list" | "--status")
            });
            let mutation = args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-i" | "--install"
                        | "-r"
                        | "--remove"
                        | "-P"
                        | "--purge"
                        | "--configure"
                        | "--unpack"
                        | "--set-selections"
                        | "--clear-selections"
                )
            });
            query && !mutation
        }
        "rpm" => {
            let query = args
                .first()
                .is_some_and(|arg| matches!(arg.as_str(), "-qa" | "-qi" | "-ql" | "--query"));
            let mutation = args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-i" | "-U"
                        | "-F"
                        | "-e"
                        | "--install"
                        | "--upgrade"
                        | "--freshen"
                        | "--erase"
                        | "--import"
                        | "--setperms"
                        | "--setugids"
                )
            });
            query && !mutation
        }

        // 容器只读
        "docker" => args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "ps" | "logs" | "inspect" | "images" | "version" | "info" | "stats" | "top"
            )
        }),

        _ => false,
    }
}

fn evaluate_write_access(parsed: &ParsedCommand, raw: &str) -> PolicyDecision {
    if let Some(reason) = find_catastrophic(parsed, raw) {
        return deny(reason);
    }
    PolicyDecision::Allow
}

/// 全局提权程序。被包装器或解释器间接调用时也会被递归检查。
fn find_privilege_escalation(parsed: &ParsedCommand) -> Option<String> {
    parsed
        .segments
        .iter()
        .find_map(|segment| privilege_in_tokens(segment))
}

fn privilege_in_tokens(tokens: &[String]) -> Option<String> {
    let tokens = shell::skip_assignments(tokens);
    let program_raw = tokens.first()?;
    let program = shell::program_name(program_raw);
    if is_privilege_program(&program) {
        return Some(program);
    }
    let rest = &tokens[1..];

    match program.as_str() {
        "env" => privilege_in_tokens(skip_env(rest)),
        "command" => {
            if rest.first().is_some_and(|arg| arg == "-v" || arg == "-V") {
                None
            } else {
                privilege_in_tokens(rest)
            }
        }
        "exec" | "nohup" | "setsid" => privilege_in_tokens(rest),
        "timeout" => privilege_in_tokens(skip_timeout(rest)),
        "nice" => privilege_in_tokens(skip_nice(rest)),
        "ionice" | "stdbuf" => privilege_in_tokens(skip_options(rest)),
        "xargs" => privilege_in_tokens(skip_xargs(rest)),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "python" | "python3" | "perl" | "ruby"
        | "node" | "php" | "eval" => find_flag_argument(rest, "-c")
            .and_then(|script| shell::parse(script).ok())
            .and_then(|parsed| {
                parsed
                    .segments
                    .iter()
                    .find_map(|segment| privilege_in_tokens(segment))
            }),
        "find" => find_flag_argument(rest, "-exec")
            .or_else(|| find_flag_argument(rest, "-execdir"))
            .and_then(|inner| shell::parse(inner).ok())
            .and_then(|parsed| {
                parsed
                    .segments
                    .iter()
                    .find_map(|segment| privilege_in_tokens(segment))
            }),
        _ => None,
    }
}

fn is_privilege_program(program: &str) -> bool {
    matches!(
        program,
        "sudo"
            | "su"
            | "doas"
            | "pkexec"
            | "runuser"
            | "setpriv"
            | "sudoedit"
            | "systemd-run"
            | "nsenter"
            | "unshare"
            | "chroot"
            | "wsl.exe"
            | "powershell.exe"
            | "cmd.exe"
            | "busybox"
    )
}

fn find_catastrophic(parsed: &ParsedCommand, raw: &str) -> Option<String> {
    for segment in &parsed.segments {
        let tokens = shell::skip_assignments(segment);
        let Some(program_raw) = tokens.first() else {
            continue;
        };
        let program = shell::program_name(program_raw);
        let args = &tokens[1..];

        match program.as_str() {
            "rm" => {
                if has_recursive_flag(args) && args.iter().any(|arg| is_protected_path(arg)) {
                    return Some("禁止递归删除系统目录或用户主目录".into());
                }
            }
            "mkfs" => return Some("禁止格式化文件系统".into()),
            "dd" => {
                if args.iter().any(|arg| arg.starts_with("of=/dev/")) {
                    return Some("禁止直接写入块设备".into());
                }
            }
            "wipefs" | "fdisk" | "parted" | "sgdisk" | "sfdisk" | "cfdisk" => {
                return Some(format!("禁止执行磁盘分区/擦除命令：{program}"));
            }
            "shutdown" | "reboot" | "poweroff" | "halt" => {
                return Some(format!("禁止执行关机/重启命令：{program}"));
            }
            "init" => {
                if args.first().is_some_and(|arg| arg == "0" || arg == "6") {
                    return Some("禁止切换运行级别 0/6".into());
                }
            }
            "chmod" | "chown" => {
                if has_recursive_flag(args) && args.iter().any(|arg| is_protected_path(arg)) {
                    return Some("禁止递归修改系统目录权限".into());
                }
            }
            "mv" => {
                if args.iter().any(|arg| arg.trim() == "/") {
                    return Some("禁止移动根目录".into());
                }
            }
            "docker" | "podman" if args.first().is_some_and(|arg| arg == "run") => {
                let privileged = args.iter().any(|arg| arg == "--privileged");
                let host_root_mount = args.windows(2).any(|window| {
                    matches!(window[0].as_str(), "-v" | "--volume") && window[1].starts_with("/:")
                });
                if privileged || host_root_mount {
                    return Some("禁止以特权模式或宿主根目录挂载运行容器".into());
                }
            }
            _ => {}
        }

        if program.starts_with("mkfs.") {
            return Some("禁止格式化文件系统".into());
        }
    }

    let compact: String = raw.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.contains(":(){:|:&};:") {
        return Some("禁止 fork bomb".into());
    }
    if compact.contains(">/dev/sd")
        || compact.contains(">/dev/nvme")
        || compact.contains(">/dev/hd")
    {
        return Some("禁止直接覆写块设备".into());
    }
    None
}

fn has_recursive_flag(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--recursive"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && (arg.contains('r') || arg.contains('R')))
    })
}

fn is_protected_path(raw: &str) -> bool {
    let path = raw
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .trim_end_matches('/');
    if path.is_empty()
        || path == "/"
        || path == "/*"
        || path == "~"
        || path == "~/*"
        || path == "."
        || path == ".."
    {
        return true;
    }

    const SYSTEM: &[&str] = &[
        "/etc", "/usr", "/boot", "/dev", "/proc", "/sys", "/bin", "/sbin", "/lib", "/lib64",
    ];
    if SYSTEM
        .iter()
        .any(|root| path == *root || path.starts_with(&format!("{root}/")))
    {
        return true;
    }

    const SHALLOW: &[&str] = &["/home", "/var", "/root", "/opt"];
    SHALLOW.iter().any(|root| {
        if path == *root {
            return true;
        }
        let Some(rest) = path.strip_prefix(root) else {
            return false;
        };
        if !rest.starts_with('/') {
            return false;
        }
        let depth = rest
            .trim_start_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
            .count();
        depth <= 1
    })
}

// ---------------- 包装器参数跳过 ----------------

fn skip_env(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--" {
            return &tokens[index + 1..];
        }
        if token == "-u" || token == "--unset" {
            index += 2;
            continue;
        }
        if token.starts_with("--unset=")
            || matches!(
                token.as_str(),
                "-i" | "--ignore-environment" | "-0" | "--null"
            )
            || token.contains('=')
        {
            index += 1;
            continue;
        }
        return &tokens[index..];
    }
    &[]
}

fn skip_timeout(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--" {
            return &tokens[index + 1..];
        }
        if matches!(token.as_str(), "-s" | "--signal" | "-k" | "--kill-after") {
            index += 2;
            continue;
        }
        if token.starts_with('-') {
            index += 1;
            continue;
        }
        // 第一个非选项是 duration，之后才是命令。
        return &tokens[index + 1..];
    }
    &[]
}

fn skip_nice(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "-n" || token == "--adjustment" {
            index += 2;
            continue;
        }
        if token.starts_with("--adjustment=") || (token.starts_with('-') && token.len() > 1) {
            index += 1;
            continue;
        }
        return &tokens[index..];
    }
    &[]
}

fn skip_options(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == "--" {
            return &tokens[index + 1..];
        }
        if tokens[index].starts_with('-') {
            index += 1;
            continue;
        }
        return &tokens[index..];
    }
    &[]
}

fn skip_xargs(tokens: &[String]) -> &[String] {
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--" {
            return &tokens[index + 1..];
        }
        if matches!(
            token.as_str(),
            "-n" | "-I" | "-P" | "-d" | "-s" | "-L" | "-E" | "-a"
        ) {
            index += 2;
            continue;
        }
        if token.starts_with('-') {
            index += 1;
            continue;
        }
        return &tokens[index..];
    }
    &[]
}

fn find_flag_argument<'a>(tokens: &'a [String], flag: &str) -> Option<&'a str> {
    let index = tokens.iter().position(|token| token == flag)?;
    tokens.get(index + 1).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(permission: TokenPermission, command: &str) -> bool {
        CommandGuard::evaluate(permission, command) == PolicyDecision::Allow
    }

    #[test]
    fn read_only_allows_diagnostics() {
        for command in [
            "ps aux",
            "ls -la",
            "journalctl -n 50",
            "docker ps",
            "ps aux | grep nginx",
            "echo hello",
            "ip addr show",
            "iptables -L -n",
            "dpkg -l",
            "rpm -qa",
            "crontab -l",
            "diff a b",
            "file /etc/hosts",
            "tree /tmp",
            "rg --pre",
        ] {
            let expected = command != "rg --pre";
            assert_eq!(
                allow(TokenPermission::ReadOnly, command),
                expected,
                "{command}"
            );
        }
    }

    #[test]
    fn read_only_rejects_writes_and_shell_tricks() {
        for command in [
            "rm -rf /tmp/x",
            "touch /tmp/x",
            "echo hi > /tmp/x",
            "cat /etc/hosts > /tmp/hosts",
            "sed -i 's/a/b/' /tmp/x",
            "tee /tmp/x",
            "dd if=/dev/zero of=/tmp/x",
            "echo $(whoami)",
            "echo `whoami`",
            "bash -c 'echo hi'",
            "python3 -c 'print(1)'",
            "find / -exec rm {} \\;",
            "xargs rm",
            "env rm -rf /tmp/x",
            "sudo ls",
            "s\"u\"do ls",
            "/usr/bin/sudo ls",
            "ip addr add 192.0.2.1/32 dev lo",
            "ip route add default via 192.0.2.1",
            "journalctl --vacuum-size=1G",
            "dmesg -cH",
            "iptables -L -F",
            "dpkg -l --configure",
            "rpm -qa -e foo",
            "crontab -l -r",
            "find /tmp -fprintf /tmp/out %p",
            "diff --output=/tmp/patch a b",
            "file -C -m /tmp/magic",
            "tree -o /tmp/tree.txt /tmp",
        ] {
            assert!(
                !allow(TokenPermission::ReadOnly, command),
                "{command} 应被拒绝"
            );
        }
    }

    #[test]
    fn write_access_allows_normal_writes_but_blocks_catastrophic() {
        for command in [
            "touch /tmp/x",
            "mkdir -p /tmp/build",
            "rm -rf /tmp/build",
            "systemctl restart nginx",
            "mv /tmp/a /tmp/b",
        ] {
            assert!(allow(TokenPermission::WriteAccess, command), "{command}");
        }
        for command in [
            "rm -rf /",
            "rm -rf /*",
            "rm -rf /etc",
            "rm -rf /home/guang",
            "mkfs.ext4 /dev/sdb1",
            "dd if=/dev/zero of=/dev/sda",
            "shutdown -h now",
            "chmod -R 777 /etc",
            "docker run --privileged ubuntu",
            "docker run -v /:/host ubuntu",
        ] {
            assert!(
                !allow(TokenPermission::WriteAccess, command),
                "{command} 应被拒绝"
            );
        }
    }

    #[test]
    fn privilege_escalation_is_blocked_for_both_permissions() {
        for permission in [TokenPermission::ReadOnly, TokenPermission::WriteAccess] {
            for command in [
                "sudo ls",
                "su -",
                "doas ls",
                "pkexec ls",
                "env sudo ls",
                "command sudo ls",
                "timeout 5 sudo ls",
                "bash -c 'sudo ls'",
                "xargs sudo ls",
                "/usr/bin/sudo ls",
                "s\"u\"do ls",
            ] {
                assert!(
                    !allow(permission, command),
                    "{permission:?} 下 {command} 应被拒绝"
                );
            }
        }
    }

    #[test]
    fn write_access_allows_documented_cleanup_paths() {
        assert!(allow(TokenPermission::WriteAccess, "rm -rf ./build"));
        assert!(allow(TokenPermission::WriteAccess, "rm -rf /tmp/build"));
        assert!(allow(
            TokenPermission::WriteAccess,
            "rm -rf /home/guang/project/node_modules"
        ));
    }

    #[test]
    fn execution_mode_parses_canonical_names() {
        assert_eq!(
            "ReadOnly".parse::<ExecutionMode>(),
            Ok(ExecutionMode::ReadOnly)
        );
        assert_eq!(
            "Restricted".parse::<ExecutionMode>(),
            Ok(ExecutionMode::Restricted)
        );
        assert_eq!(
            "FullAccess".parse::<ExecutionMode>(),
            Ok(ExecutionMode::FullAccess)
        );
        assert!("nope".parse::<ExecutionMode>().is_err());
    }
}
