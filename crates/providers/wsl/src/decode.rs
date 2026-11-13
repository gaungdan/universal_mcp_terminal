//! `wsl.exe` 输出解码。
//!
//! `wsl.exe -l -q` 等管理命令在 Windows 上输出 **UTF-16LE**（中文错误信息同样如此），
//! 而发行版内命令（bash 等）的输出是 UTF-8。这里用启发式规则区分两者，
//! 避免发行版名称或错误信息变成乱码。

/// 判断字节流是否应按 UTF-16LE 解码。
///
/// 规则（按优先级）：
/// 1. 以 UTF-16LE BOM（`FF FE`）开头；
/// 2. 长度为偶数且包含 NUL 字节 —— ASCII 为主的 UTF-16LE 文本每个码元高位都是 0；
/// 3. 不是合法 UTF-8 —— UTF-16LE 的中文文本通常不是合法 UTF-8；
/// 4. 是合法 UTF-8 但包含除 `\t` `\r` `\n` 之外的控制字符 —— UTF-16LE 中文被误判为
///    UTF-8 时会出现这类控制字符。
pub(crate) fn looks_like_utf16le(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return true;
    }
    if !bytes.len().is_multiple_of(2) {
        return false;
    }
    if bytes.contains(&0) {
        return true;
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\t' | '\r' | '\n')),
        Err(_) => true,
    }
}

/// 按 UTF-16LE 解码，去掉可选的 BOM 与尾部/中间 NUL。
pub(crate) fn decode_utf16le(bytes: &[u8]) -> String {
    let payload = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
    let (pairs, _remainder) = payload.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    String::from_utf16_lossy(&units)
        .chars()
        .filter(|ch| *ch != '\0')
        .collect()
}

/// 解码 `wsl.exe` 的任意输出：能识别 UTF-16LE，其余按 UTF-8 lossy 处理。
pub(crate) fn decode_output(bytes: &[u8]) -> String {
    let mut text = if looks_like_utf16le(bytes) {
        decode_utf16le(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    if text.starts_with('\u{feff}') {
        text.remove(0);
    }
    text
}

/// 按行切分并过滤空行，用于解析 `wsl.exe -l -q` 的发行版列表。
pub(crate) fn split_lines(text: &str) -> Vec<String> {
    text.split(['\r', '\n'])
        .map(|line| line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\0'))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    #[test]
    fn decodes_utf16le_distribution_list() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(utf16le("Ubuntu\r\nDebian\r\n\n"));
        let text = decode_output(&bytes);
        assert_eq!(split_lines(&text), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn decodes_utf16le_without_bom() {
        let bytes = utf16le("docker-desktop\r\n");
        assert!(looks_like_utf16le(&bytes));
        assert_eq!(split_lines(&decode_output(&bytes)), vec!["docker-desktop"]);
    }

    #[test]
    fn keeps_utf8_output_intact() {
        let bytes = "Linux admin 6.18.0-microsoft-standard-WSL2 x86_64 GNU/Linux\n".as_bytes();
        assert!(!looks_like_utf16le(bytes));
        assert_eq!(
            decode_output(bytes).trim(),
            "Linux admin 6.18.0-microsoft-standard-WSL2 x86_64 GNU/Linux"
        );
    }

    #[test]
    fn decodes_utf16le_chinese_error_text() {
        // 中文 UTF-16LE 一般不是合法 UTF-8，必须按 UTF-16LE 解码
        let bytes = utf16le("没有找到发行版 WSL_E_DISTRO_NOT_FOUND\r\n");
        assert!(looks_like_utf16le(&bytes));
        assert!(decode_output(&bytes).contains("WSL_E_DISTRO_NOT_FOUND"));
        assert!(decode_output(&bytes).contains("没有找到发行版"));
    }

    #[test]
    fn split_lines_ignores_blank_and_nul() {
        assert_eq!(
            split_lines("  \r\n\r\nUbuntu\n\0\n  Debian  \n"),
            vec!["Ubuntu", "Debian"]
        );
    }
}
