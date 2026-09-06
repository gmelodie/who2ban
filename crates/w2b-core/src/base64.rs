const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .chain([0, 0].iter())
            .take(3)
            .fold(0u32, |word, byte| word << 8 | u32::from(*byte));
        for i in 0..4 {
            match i <= chunk.len() {
                true => out.push(ALPHABET[(word >> (18 - 6 * i)) as usize & 63] as char),
                false => out.push('='),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tail_is_padded() {
        assert_eq!(encode(b"a:b"), "YTpi");
        assert_eq!(encode(b"a:bc"), "YTpiYw==");
        assert_eq!(encode(b"a:bcd"), "YTpiY2Q=");
        assert_eq!(encode(b"me:secret"), "bWU6c2VjcmV0");
    }
}
