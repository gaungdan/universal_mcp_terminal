//! Telnet 线协议的最小实现：IAC 协商过滤与分页提示识别。
//!
//! 这里刻意不实现完整的 Telnet 选项集：只需要避免 `0xFF` 命令序列污染终端输出，
//! 并以 WONT 拒绝服务端提出的所有选项。

/// IAC（Interpret As Command）起始字节。
const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
/// TERMINAL-TYPE 选项号。
const TTYPE: u8 = 24;
/// TERMINAL-TYPE 子协商中的 SEND 子命令。
const TTYPE_SEND: u8 = 1;
/// TERMINAL-TYPE 子协商中的 IS 子命令。
const TTYPE_IS: u8 = 0;

#[derive(Debug, Clone, Copy)]
enum State {
    Data,
    Iac,
    Negotiation,
    Subnegotiation,
    SubnegotiationIac,
}

/// 增量 Telnet 解码器。
///
/// `process` 可以按任意大小的网络分片反复调用：未完成的 IAC 序列会保留在状态机里。
#[derive(Debug, Clone)]
pub struct TelnetDecoder {
    state: State,
    subnegotiation: Vec<u8>,
    terminal_type: Vec<u8>,
}

impl TelnetDecoder {
    pub fn new(terminal_type: impl AsRef<str>) -> Self {
        let terminal_type = terminal_type
            .as_ref()
            .bytes()
            .filter(u8::is_ascii_graphic)
            .collect();
        Self {
            state: State::Data,
            subnegotiation: Vec::new(),
            terminal_type,
        }
    }

    /// 处理一段网络数据：`data` 为过滤后的终端字节，`response` 为需要回写给服务端的字节。
    pub fn process(&mut self, input: &[u8], data: &mut Vec<u8>, response: &mut Vec<u8>) {
        for &byte in input {
            self.feed(byte, data, response);
        }
    }

    fn feed(&mut self, byte: u8, data: &mut Vec<u8>, response: &mut Vec<u8>) {
        match self.state {
            State::Data => {
                if byte == IAC {
                    self.state = State::Iac;
                } else {
                    data.push(byte);
                }
            }
            State::Iac => match byte {
                IAC => {
                    // IAC IAC 表示一个字面量 0xFF。
                    data.push(IAC);
                    self.state = State::Data;
                }
                WILL | WONT | DO | DONT => {
                    self.state = State::Negotiation;
                }
                SB => {
                    self.subnegotiation.clear();
                    self.state = State::Subnegotiation;
                }
                // NOP、GA、AYT 等单字节命令直接忽略。
                _ => self.state = State::Data,
            },
            State::Negotiation => {
                // 统一拒绝所有选项：这样服务端不会误以为我们接受了回显等选项而改变行为。
                response.extend_from_slice(&[IAC, WONT, byte]);
                self.state = State::Data;
            }
            State::Subnegotiation => {
                if byte == IAC {
                    self.state = State::SubnegotiationIac;
                } else {
                    self.subnegotiation.push(byte);
                }
            }
            State::SubnegotiationIac => {
                if byte == SE {
                    self.answer_subnegotiation(response);
                    self.subnegotiation.clear();
                    self.state = State::Data;
                } else if byte == IAC {
                    self.subnegotiation.push(IAC);
                    self.state = State::Subnegotiation;
                } else {
                    // 非法的子协商转义，丢弃并继续解析后续数据。
                    self.state = State::Subnegotiation;
                }
            }
        }
    }

    fn answer_subnegotiation(&self, response: &mut Vec<u8>) {
        // 只有 TERMINAL-TYPE SEND 需要回应，其余子协商一律忽略。
        if self.subnegotiation.len() >= 2
            && self.subnegotiation[0] == TTYPE
            && self.subnegotiation[1] == TTYPE_SEND
        {
            response.extend_from_slice(&[IAC, SB, TTYPE, TTYPE_IS]);
            response.extend_from_slice(&self.terminal_type);
            response.extend_from_slice(&[IAC, SE]);
        }
    }
}

/// 判断文本尾部是否出现常见分页提示。
///
/// 支持 `--More--`、`---- More ----`、`<--- More --->`、`(more)` 及其大小写变体。
pub fn detect_pager(text: &str) -> bool {
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    let lower = cleaned.to_ascii_lowercase();
    let skip = lower.chars().count().saturating_sub(80);
    let tail: String = lower.chars().skip(skip).collect();
    // 去掉空白后再匹配，兼容设备在提示中插入的对齐空格。
    let compact: String = tail.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("--more--") || compact.contains("(more)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_iac_negotiation_and_replies_wont() {
        let mut decoder = TelnetDecoder::new("xterm");
        let mut data = Vec::new();
        let mut response = Vec::new();

        decoder.process(&[b'a', IAC, WILL, 1, b'b'], &mut data, &mut response);

        assert_eq!(data, b"ab");
        assert_eq!(response, [IAC, WONT, 1]);
    }

    #[test]
    fn treats_doubled_iac_as_literal_byte() {
        let mut decoder = TelnetDecoder::new("xterm");
        let mut data = Vec::new();
        let mut response = Vec::new();

        decoder.process(&[b'x', IAC, IAC, b'y'], &mut data, &mut response);

        assert_eq!(data, [b'x', 0xFF, b'y']);
        assert!(response.is_empty());
    }

    #[test]
    fn answers_terminal_type_subnegotiation() {
        let mut decoder = TelnetDecoder::new("vt100");
        let mut data = Vec::new();
        let mut response = Vec::new();

        decoder.process(
            &[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE, b'z'],
            &mut data,
            &mut response,
        );

        assert_eq!(data, b"z");
        let mut expected = vec![IAC, SB, TTYPE, TTYPE_IS];
        expected.extend_from_slice(b"vt100");
        expected.extend_from_slice(&[IAC, SE]);
        assert_eq!(response, expected);
    }

    #[test]
    fn detects_common_pager_prompts() {
        assert!(detect_pager("line\r\n--More--"));
        assert!(detect_pager("line\r\n---- More ----"));
        assert!(detect_pager("line\r\n<--- More --->"));
        assert!(detect_pager("line\r\n--MORE--"));
        assert!(detect_pager("line\r\n(more)"));
        assert!(!detect_pager("line\r\nRouter# "));
        assert!(!detect_pager(""));
    }
}
