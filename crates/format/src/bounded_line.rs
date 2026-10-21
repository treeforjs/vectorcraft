use std::io::{self, BufRead};

/// Read one line without letting an unterminated input grow the allocation without limit.
/// `max` excludes LF or CRLF; EOF also terminates a nonempty line. On error the caller must
/// close the input rather than interpreting the remaining bytes as another request.
pub fn read_bounded_line(reader: &mut impl BufRead, bytes: &mut Vec<u8>, max: usize) -> io::Result<bool> {
    bytes.clear();
    let wire_limit = max.checked_add(2).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "line limit is too large"))?;
    let too_long = || io::Error::new(io::ErrorKind::InvalidData, format!("request line longer than {max} bytes; closing the connection"));
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if bytes.len() > max { Err(too_long()) } else { Ok(!bytes.is_empty()) };
        }
        let remaining = wire_limit.saturating_sub(bytes.len());
        let chunk = available.get(..available.len().min(remaining)).ok_or_else(too_long)?;
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(chunk.len(), |at| at + 1);
        let chunk = chunk.get(..take).ok_or_else(too_long)?;
        bytes.try_reserve(take).map_err(|e| io::Error::new(io::ErrorKind::OutOfMemory, e))?;
        bytes.extend_from_slice(chunk);
        reader.consume(take);
        if newline.is_some() {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return if bytes.len() > max { Err(too_long()) } else { Ok(true) };
        }
        if bytes.len() == wire_limit {
            return Err(too_long());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn exact_limit_accepts_lf_crlf_and_eof() {
        for mut input in [&b"12345678\nnext\n"[..], &b"12345678\r\nnext\n"[..], &b"12345678"[..]] {
            let mut bytes = Vec::new();
            assert!(read_bounded_line(&mut input, &mut bytes, 8).unwrap());
            assert_eq!(bytes, b"12345678");
            if !input.is_empty() {
                assert!(read_bounded_line(&mut input, &mut bytes, 8).unwrap());
                assert_eq!(bytes, b"next");
            }
            assert!(!read_bounded_line(&mut input, &mut bytes, 8).unwrap());
        }
    }

    #[test]
    fn oversized_line_stops_before_later_input_and_bounds_the_buffer() {
        for mut input in [&b"123456789\nnext\n"[..], &b"123456789\r\nnext\n"[..], &b"123456789"[..]] {
            let mut bytes = Vec::new();
            assert_eq!(read_bounded_line(&mut input, &mut bytes, 8).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert!(bytes.len() <= 10);
        }
    }

    #[test]
    fn split_crlf_and_empty_lines_have_the_same_limits() {
        let mut input = BufReader::with_capacity(1, &b"\r\n\n12345678\r\n"[..]);
        let mut bytes = Vec::new();
        for _ in 0..2 {
            assert!(read_bounded_line(&mut input, &mut bytes, 0).unwrap());
            assert!(bytes.is_empty());
        }
        assert!(read_bounded_line(&mut input, &mut bytes, 8).unwrap());
        assert_eq!(bytes, b"12345678");
    }

    #[test]
    fn impossible_limit_is_rejected_before_reading() {
        assert_eq!(read_bounded_line(&mut &b"x"[..], &mut Vec::new(), usize::MAX).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
