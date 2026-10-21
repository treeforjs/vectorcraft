//! Bounded reads of document, image and other local input files.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read as _};
use std::path::Path;

/// Largest local input file accepted (2 GiB); decoded formats have their own tighter limits.
pub const MAX_FILE_BYTES: u64 = 2 << 30;

/// Read a regular file, rejecting devices, pipes and inputs larger than [`MAX_FILE_BYTES`].
pub fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    read_with_limit(path, MAX_FILE_BYTES)
}

/// Read at most `max_bytes` from a regular input for format detection. The complete file must
/// still fit [`MAX_FILE_BYTES`]; callers can skip unknown formats without loading their body.
pub fn read_regular_file_prefix(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    read_prefix_with_limit(path, max_bytes, MAX_FILE_BYTES)
}

fn read_prefix_with_limit(path: &Path, max_bytes: usize, limit: u64) -> io::Result<Vec<u8>> {
    let file = open_regular_file(path, limit)?;
    let prefix_bytes = (max_bytes as u64).min(limit);
    read_capped(file.take(prefix_bytes), prefix_bytes)
}

fn check_metadata(metadata: &Metadata, limit: u64) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "input must be a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("input exceeds the {limit}-byte file limit")));
    }
    Ok(())
}

fn read_with_limit(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    read_capped(open_regular_file(path, limit)?, limit)
}

fn open_regular_file(path: &Path, limit: u64) -> io::Result<File> {
    check_metadata(&std::fs::metadata(path)?, limit)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // Metadata and open are separate operations: never wait for a FIFO swapped into the path.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    check_metadata(&file.metadata()?, limit)?;
    Ok(file)
}

fn read_capped(reader: impl io::Read, limit: u64) -> io::Result<Vec<u8>> {
    let probe_limit = limit.checked_add(1).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file limit is too large"))?;
    let mut reader = reader.take(probe_limit);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(bytes);
        }
        let next = bytes.len().checked_add(n).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "input size overflow"))?;
        if next as u64 > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("input exceeds the {limit}-byte file limit")));
        }
        bytes.try_reserve(n).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "not enough memory to read the input file"))?;
        let read = chunk.get(..n).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid input read size"))?;
        bytes.extend_from_slice(read);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_is_accepted_and_growth_past_the_limit_is_rejected() {
        assert_eq!(read_capped(&b"12345678"[..], 8).unwrap(), b"12345678");
        assert_eq!(read_capped(&b"123456789"[..], 8).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(read_capped(&b""[..], 0).unwrap().is_empty());
    }

    #[test]
    fn reads_regular_files_but_rejects_large_inputs_and_directories() {
        let dir = std::env::temp_dir().join(format!("vc-bounded-read-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("input");
        std::fs::write(&path, b"12345678").unwrap();
        assert_eq!(read_with_limit(&path, 8).unwrap(), b"12345678");
        assert_eq!(read_with_limit(&path, 7).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(read_with_limit(&dir, 8).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prefix_sniffing_is_bounded_and_still_refuses_invalid_complete_inputs() {
        let dir = std::env::temp_dir().join(format!("vc-bounded-prefix-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("input");
        std::fs::write(&path, b"12345678").unwrap();
        assert_eq!(read_regular_file_prefix(&path, 4).unwrap(), b"1234");
        assert_eq!(read_prefix_with_limit(&path, 4, 8).unwrap(), b"1234");
        assert!(read_regular_file_prefix(&path, 0).unwrap().is_empty());
        assert_eq!(read_prefix_with_limit(&path, 4, 7).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(read_regular_file_prefix(&dir, 4).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_devices_and_fifos_without_waiting_for_a_writer() {
        assert_eq!(read_regular_file(Path::new("/dev/zero")).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(read_regular_file_prefix(Path::new("/dev/zero"), 4).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        let path = std::env::temp_dir().join(format!("vc-input-fifo-{}", std::process::id()));
        assert!(std::process::Command::new("mkfifo").arg(&path).status().unwrap().success());
        assert_eq!(read_regular_file(&path).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(read_regular_file_prefix(&path, 4).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_file(&path).unwrap();
    }
}
