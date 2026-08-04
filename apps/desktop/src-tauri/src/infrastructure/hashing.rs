use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use crate::domain::CoreError;

pub fn sha256_file(path: &Path) -> Result<(String, u64), CoreError> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn hashes_file_content_and_size() {
        let mut file = NamedTempFile::new().expect("temporary file");
        file.write_all(b"vietdub").expect("write fixture");

        let (hash, size) = sha256_file(file.path()).expect("hash file");

        assert_eq!(size, 7);
        assert_eq!(
            hash,
            "3c05dd49ddbe3276a0eade6f107b88c88cf093363d4bdeac95f24f66cc484f28"
        );
    }
}
