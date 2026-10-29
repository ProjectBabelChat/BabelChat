//! What a file is, from its first bytes: never from its name or from what
//! the uploader claims, since browsers act on the type a server gives.

/// Types shown as themselves: images every client can draw. Anything else
/// is served as a download.
pub const INLINE: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Leading bytes and the type they mean. `?` matches any byte.
const SIGNATURES: &[(&[u8], &str)] = &[
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"GIF87a", "image/gif"),
    (b"GIF89a", "image/gif"),
    (b"RIFF????WEBP", "image/webp"),
    (b"%PDF-", "application/pdf"),
    (b"PK\x03\x04", "application/zip"),
    (b"????ftyp", "video/mp4"),
    (b"\x1a\x45\xdf\xa3", "video/webm"),
    (b"ID3", "audio/mpeg"),
    (b"OggS", "audio/ogg"),
    (b"RIFF????WAVE", "audio/wav"),
];

/// The file's type, from `head` (its first bytes).
pub fn content_type(head: &[u8]) -> &'static str {
    SIGNATURES
        .iter()
        .find(|(signature, _)| {
            head.len() >= signature.len()
                && signature
                    .iter()
                    .zip(head)
                    .all(|(expected, byte)| *expected == b'?' || expected == byte)
        })
        .map(|(_, content_type)| *content_type)
        .unwrap_or("application/octet-stream")
}

/// Width and height of an image shown inline, from its header.
pub fn dimensions(content_type: &str, head: &[u8]) -> Option<(i64, i64)> {
    if !INLINE.contains(&content_type) {
        return None;
    }
    let size = imagesize::blob_size(head).ok()?;
    Some((size.width as i64, size.height as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_known_by_their_first_bytes() {
        let png = [
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            0, 0, 1, 0, 0, 0, 0, 200, 8, 6, 0, 0, 0,
        ];
        assert_eq!(content_type(&png), "image/png");
        assert_eq!(dimensions("image/png", &png), Some((256, 200)));
        assert_eq!(content_type(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(content_type(b"RIFF\0\0\0\0WAVEfmt "), "audio/wav");
        assert_eq!(content_type(b"%PDF-1.7"), "application/pdf");
        // A page that calls itself an image is still a download.
        assert_eq!(
            content_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            "application/octet-stream"
        );
        assert_eq!(dimensions("application/pdf", b"%PDF-1.7"), None);
    }
}
