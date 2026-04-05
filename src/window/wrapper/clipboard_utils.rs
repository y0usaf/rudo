use crate::terminal::ClipboardSelection;

pub(crate) fn clipboard_register(selection: ClipboardSelection) -> &'static str {
    match selection {
        ClipboardSelection::Clipboard => "+",
        #[cfg(target_os = "linux")]
        ClipboardSelection::Primary => "*",
        #[cfg(not(target_os = "linux"))]
        ClipboardSelection::Primary => "+",
        _ => "+",
    }
}

pub(crate) fn encode_osc52_reply(selection: ClipboardSelection, content: &str) -> Vec<u8> {
    let selection_code = match selection {
        ClipboardSelection::Clipboard => 'c',
        ClipboardSelection::Primary => 'p',
        ClipboardSelection::Secondary => 'q',
        ClipboardSelection::Select => 's',
        ClipboardSelection::Cut0 => '0',
        ClipboardSelection::Cut1 => '1',
        ClipboardSelection::Cut2 => '2',
        ClipboardSelection::Cut3 => '3',
        ClipboardSelection::Cut4 => '4',
        ClipboardSelection::Cut5 => '5',
        ClipboardSelection::Cut6 => '6',
        ClipboardSelection::Cut7 => '7',
    };
    let encoded = encode_base64(content.as_bytes());
    format!("\x1b]52;{selection_code};{encoded}\x1b\\").into_bytes()
}

pub(crate) fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = u32::from(b0) << 16 | u32::from(b1) << 8 | u32::from(b2);

        output.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}
