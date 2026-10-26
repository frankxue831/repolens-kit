use sha2::{Digest, Sha256};

pub struct ScanConfigHash;

impl ScanConfigHash {
    /// Compute SHA-256 config hash matching Java's ScanConfigHash byte layout.
    ///
    /// Layout: for each rules content → 8-byte BE length + bytes,
    /// then follow_symlinks (1 byte), include_binaries (1 byte),
    /// max_file_bytes (8-byte BE i64), max_total_bytes (8-byte BE i64).
    pub fn compute(
        rules_contents: &[Vec<u8>],
        follow_symlinks: bool,
        include_binaries: bool,
        max_file_bytes: i64,
        max_total_bytes: i64,
    ) -> String {
        let mut hasher = Sha256::new();

        for content in rules_contents {
            hasher.update((content.len() as i64).to_be_bytes());
            hasher.update(content);
        }

        hasher.update([if follow_symlinks { 1u8 } else { 0u8 }]);
        hasher.update([if include_binaries { 1u8 } else { 0u8 }]);
        hasher.update(max_file_bytes.to_be_bytes());
        hasher.update(max_total_bytes.to_be_bytes());

        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_default_options() {
        let hash = ScanConfigHash::compute(&[], false, false, 524_288, 52_428_800);
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        let hash2 = ScanConfigHash::compute(&[], false, false, 524_288, 52_428_800);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn different_options_produce_different_hash() {
        let h1 = ScanConfigHash::compute(&[], false, false, 524_288, 52_428_800);
        let h2 = ScanConfigHash::compute(&[], true, false, 524_288, 52_428_800);
        let h3 = ScanConfigHash::compute(&[], false, true, 524_288, 52_428_800);
        let h4 = ScanConfigHash::compute(&[], false, false, 1_000_000, 52_428_800);
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h1, h4);
    }

    #[test]
    fn rules_content_affects_hash() {
        let h1 = ScanConfigHash::compute(&[], false, false, 524_288, 52_428_800);
        let h2 = ScanConfigHash::compute(&[b"*.log\n".to_vec()], false, false, 524_288, 52_428_800);
        assert_ne!(h1, h2);
    }

    #[test]
    fn multiple_rules_files_order_matters() {
        let h1 = ScanConfigHash::compute(
            &[b"*.log\n".to_vec(), b"*.tmp\n".to_vec()],
            false,
            false,
            524_288,
            52_428_800,
        );
        let h2 = ScanConfigHash::compute(
            &[b"*.tmp\n".to_vec(), b"*.log\n".to_vec()],
            false,
            false,
            524_288,
            52_428_800,
        );
        assert_ne!(h1, h2, "order of rules files should matter");
    }

    #[test]
    fn length_prefix_prevents_boundary_collision() {
        let h1 = ScanConfigHash::compute(
            &[b"ab".to_vec(), b"cd".to_vec()],
            false,
            false,
            524_288,
            52_428_800,
        );
        let h2 = ScanConfigHash::compute(
            &[b"a".to_vec(), b"bcd".to_vec()],
            false,
            false,
            524_288,
            52_428_800,
        );
        assert_ne!(h1, h2, "length prefix should prevent boundary collisions");
    }
}
