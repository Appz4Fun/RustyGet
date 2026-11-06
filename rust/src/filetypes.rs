//! FileTypes: file and directory names and contents classified (archives,
//! parity, video, audio, subtitles, books, images, disc structures, samples,
//! clutter) and a file's type sniffed from its first bytes. It matches the
//! C++ FileTypes; names compare as Util::StrCaseCmp (the C library's tolower
//! in the current locale).

use std::ffi::{c_int, CStr};

extern "C" {
    fn tolower(c: c_int) -> c_int;
    fn isdigit(c: c_int) -> c_int;
}

fn digit(b: u8) -> bool {
    unsafe { isdigit(b as c_int) != 0 }
}

/// Util::StrCaseCmp.
pub fn str_case_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| unsafe { tolower(x as c_int) == tolower(y as c_int) })
}

fn matches_any_ext(ext: &[u8], formats: &[&str]) -> bool {
    ext.first() == Some(&b'.') && formats.iter().any(|f| str_case_eq(ext, f.as_bytes()))
}

fn any_name(name: &[u8], names: &[&str]) -> bool {
    names.iter().any(|n| str_case_eq(name, n.as_bytes()))
}

/// The part after the last '/' or '\\'.
fn basename(path: &[u8]) -> &[u8] {
    path.iter().rposition(|&b| b == b'/' || b == b'\\').map_or(path, |k| &path[k + 1..])
}

/// Without the last extension (a name starting with its only '.' stays).
fn strip_last_ext(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'.') {
        None | Some(0) => name,
        Some(k) => &name[..k],
    }
}

/// FileSystem::GetFileExtension: from the last '.' on ("" for none).
pub fn file_extension(name: &[u8]) -> &[u8] {
    name.iter().rposition(|&b| b == b'.').map_or(&[], |k| &name[k..])
}

pub fn is_seven_zip_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".7z", ".zip", ".tar", ".gz", ".bz", ".bz2", ".tgz", ".txz", ".xz"])
}

pub fn is_rar_ext(ext: &[u8]) -> bool {
    str_case_eq(ext, b".rar")
}

pub fn is_rar_volume_ext(ext: &[u8]) -> bool {
    if ext.len() != 4 || ext[0] != b'.' {
        return false;
    }
    let l = unsafe { tolower(ext[1] as c_int) };
    (b'r' as c_int..=b'z' as c_int).contains(&l) && digit(ext[2]) && digit(ext[3])
}

pub fn is_all_digits_ext(ext: &[u8]) -> bool {
    ext.len() >= 2 && ext[0] == b'.' && ext[1..].iter().all(|&c| digit(c))
}

pub fn is_numeric_volume_ext(ext: &[u8]) -> bool {
    ext.len() == 4 && is_all_digits_ext(ext)
}

pub fn is_archive_ext(ext: &[u8]) -> bool {
    is_seven_zip_ext(ext) || is_rar_ext(ext) || is_rar_volume_ext(ext) || is_numeric_volume_ext(ext)
}

pub fn is_disc_structure_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".vob", ".bdmv", ".mpls", ".mpl", ".clpi", ".cpi", ".bdm", ".ifo", ".bup", ".mts", ".m2ts", ".aob", ".evo", ".bdjo"])
}

pub fn is_disc_structure_dir(dir: &[u8]) -> bool {
    any_name(basename(dir), &["BDMV", "VIDEO_TS", "AUDIO_TS", "HVDVD_TS", "AVCHD", "CERTIFICATE"])
}

pub fn is_disc_descriptor_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".cue", ".mds", ".ccd", ".toc"])
}

pub fn is_disc_image_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".iso", ".mdf", ".nrg", ".cdi", ".gdi"])
}

pub fn is_generic_disc_image_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".bin", ".img"])
}

pub fn is_clutter_dir(dir: &[u8]) -> bool {
    any_name(basename(dir), &["@eaDir", ".AppleDouble", "__MACOSX", ".Spotlight-V100", ".Trashes"])
}

pub fn is_clutter_file(name: &[u8]) -> bool {
    let bare = basename(name);
    // AppleDouble sidecar files (._Movie.mkv)
    (bare.len() > 2 && bare[0] == b'.' && bare[1] == b'_') || any_name(bare, &[".DS_Store", "Thumbs.db", "desktop.ini", "ehthumbs.db"])
}

pub fn is_parity_ext(ext: &[u8]) -> bool {
    str_case_eq(ext, b".par2") || str_case_eq(ext, b".sfv")
}

pub fn is_video_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".mkv", ".mp4", ".avi", ".mov", ".m2ts", ".mts", ".ts", ".m4v", ".webm", ".flv", ".wmv", ".divx", ".xvid"])
}

pub fn is_audio_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".mp3", ".flac", ".aac", ".ogg", ".wav", ".dts", ".ac3", ".mka", ".opus", ".wma", ".eac3", ".m4a"])
}

pub fn is_subtitle_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".srt", ".sub", ".idx", ".ass", ".ssa", ".smi", ".sup", ".pgs", ".vtt"])
}

pub fn is_nfo_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".nfo", ".info"])
}

pub fn is_book_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".epub", ".pdf", ".mobi", ".azw3", ".cbr", ".cbz", ".djvu", ".m4b"])
}

pub fn is_image_ext(ext: &[u8]) -> bool {
    matches_any_ext(ext, &[".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".tif", ".tiff"])
}

pub fn is_sample_stem(stem: &[u8]) -> bool {
    if str_case_eq(stem, b"sample") {
        return true;
    }
    if stem.len() < 7 {
        return false;
    }
    let suffix = &stem[stem.len() - 7..];
    str_case_eq(suffix, b"-sample") || str_case_eq(suffix, b".sample") || str_case_eq(suffix, b"_sample")
}

pub fn is_seven_zip_file(name: &[u8]) -> bool {
    let bare = basename(name);
    let ext = file_extension(bare);
    if ext.is_empty() {
        return false;
    }
    if [&b".001"[..], b".gz", b".bz2", b".xz"].iter().any(|e| str_case_eq(ext, e)) {
        let inner = file_extension(strip_last_ext(bare));
        if !inner.is_empty() && is_seven_zip_ext(inner) {
            return true;
        }
    }
    is_seven_zip_ext(ext)
}

pub fn is_rar_file(name: &[u8]) -> bool {
    let bare = basename(name);
    let ext = file_extension(bare);
    if ext.is_empty() {
        return false;
    }
    if is_rar_ext(ext) || is_rar_volume_ext(ext) {
        return true;
    }
    if is_numeric_volume_ext(ext) {
        let nested = file_extension(strip_last_ext(bare));
        return !nested.is_empty() && is_rar_ext(nested);
    }
    false
}

pub fn is_archive_file(name: &[u8]) -> bool {
    is_seven_zip_file(name) || is_rar_file(name)
}

pub fn is_sample_file(name: &[u8]) -> bool {
    is_sample_stem(strip_last_ext(basename(name)))
}

pub fn is_known_collection_ext(ext: &[u8]) -> bool {
    !ext.is_empty()
        && (is_video_ext(ext)
            || is_audio_ext(ext)
            || is_subtitle_ext(ext)
            || is_nfo_ext(ext)
            || is_book_ext(ext)
            || is_image_ext(ext)
            || is_archive_ext(ext)
            || is_parity_ext(ext)
            || is_disc_structure_ext(ext)
            || is_disc_image_ext(ext)
            || is_disc_descriptor_ext(ext)
            || is_generic_disc_image_ext(ext))
}

fn at(h: &[u8], offset: usize, lit: &[u8]) -> bool {
    h.len() >= offset + lit.len() && &h[offset..offset + lit.len()] == lit
}

fn contains(h: &[u8], needle: &[u8]) -> bool {
    h.windows(needle.len()).any(|w| w == needle)
}

/// FileTypes::SniffExtension: the extension a file's first bytes show ("" if
/// none is certain), as a C string for the C++ side.
pub fn sniff_extension(h: &[u8]) -> &'static CStr {
    let n = h.len();
    if at(h, 0, b"%PDF-") {
        return c".pdf";
    }
    // EBML: MKV or WebM
    if at(h, 0, &[0x1a, 0x45, 0xdf, 0xa3]) {
        return if contains(h, b"webm") { c".webm" } else { c".mkv" };
    }
    if n >= 8 && at(h, 4, b"ftyp") {
        if n >= 12 {
            match &h[8..12] {
                b"M4V " => return c".m4v",
                b"M4A " => return c".m4a",
                b"M4B " => return c".m4b",
                b"qt  " => return c".mov",
                _ => {}
            }
        }
        return c".mp4";
    }
    if n >= 8 && at(h, 4, b"moov") {
        return c".mp4";
    }
    if n >= 12 && at(h, 0, b"RIFF") {
        match &h[8..12] {
            b"AVI " => return c".avi",
            b"WAVE" => return c".wav",
            b"WEBP" => return c".webp",
            _ => {}
        }
    }
    if at(h, 0, &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return c".png";
    }
    if at(h, 0, b"GIF87a") || at(h, 0, b"GIF89a") {
        return c".gif";
    }
    if at(h, 0, &[0xff, 0xd8, 0xff]) {
        return c".jpg";
    }
    if n >= 14 && h[0] == b'B' && h[1] == b'M' && h[6..10] == [0, 0, 0, 0] {
        return c".bmp";
    }
    // MPEG-TS: sync bytes 188 or 192 apart, three packets when the buffer has them
    if n >= 189 && h[0] == 0x47 {
        if h[188] == 0x47 && (n < 377 || h[376] == 0x47) {
            return c".ts";
        }
        if n >= 193 && h[192] == 0x47 && (n < 385 || h[384] == 0x47) {
            return c".ts";
        }
    }
    // ASF: WMV or WMA by the stream GUID, undecided without one
    if at(h, 0, &[0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11]) {
        const VIDEO: [u8; 16] = [0xc0, 0xef, 0x19, 0xbc, 0x4d, 0x5b, 0xcf, 0x11, 0xa8, 0xfd, 0x00, 0x80, 0x5f, 0x5c, 0x44, 0x2b];
        const AUDIO: [u8; 16] = [0x40, 0x9e, 0x69, 0xf8, 0x4d, 0x5b, 0xcf, 0x11, 0xa8, 0xfd, 0x00, 0x80, 0x5f, 0x5c, 0x44, 0x2b];
        if n >= 16 && contains(h, &VIDEO) {
            return c".wmv";
        }
        if n >= 16 && contains(h, &AUDIO) {
            return c".wma";
        }
        return c"";
    }
    if at(h, 0, b"fLaC") {
        return c".flac";
    }
    if at(h, 0, b"ID3") || (n >= 2 && h[0] == 0xff && (h[1] & 0xe6) == 0xe2) {
        return c".mp3";
    }
    if at(h, 0, b"OggS") {
        return c".ogg";
    }
    if at(h, 0, &[0x50, 0x4b, 0x03, 0x04]) {
        return if contains(h, b"application/epub+zip") { c".epub" } else { c".zip" };
    }
    if n >= 68 && at(h, 60, b"BOOKMOBI") {
        return c".mobi";
    }
    if n >= 16 && at(h, 0, b"AT&TFORM") && (at(h, 12, b"DJVU") || at(h, 12, b"DJVM")) {
        return c".djvu";
    }
    if at(h, 0, b"Rar!\x1a\x07") {
        return c".rar";
    }
    if at(h, 0, &[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]) {
        return c".7z";
    }
    if at(h, 0, &[0x1f, 0x8b, 0x08]) {
        return c".gz";
    }
    if at(h, 0, b"BZh") {
        return c".bz2";
    }
    if at(h, 0, &[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        return c".xz";
    }
    if n >= 262 && at(h, 257, b"ustar") {
        return c".tar";
    }
    if at(h, 0, &[0x1f, 0x9d]) {
        return c".Z";
    }
    c""
}

/// FileTypes::SniffExtension(path): reads up to 512 bytes from a regular file
/// and sniffs its extension.
pub fn sniff_extension_file(path: &std::path::Path) -> &'static CStr {
    use std::io::Read;
    let Ok(meta) = std::fs::metadata(path) else {
        return c"";
    };
    if !meta.is_file() {
        return c"";
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return c"";
    };
    let mut buf = [0u8; 512];
    let mut total = 0usize;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if total == 0 {
        return c"";
    }
    sniff_extension(&buf[..total])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_rar_file(b"dir/x.part01.rar"));
        assert!(is_rar_file(b"x.rar.001"));
        assert!(is_rar_file(b"x.r00"));
        assert!(is_seven_zip_file(b"x.7z.001"));
        assert!(is_sample_file(b"/a/Movie-sample.mkv"));
        assert!(is_clutter_file(b"._Movie.mkv"));
        assert!(!is_archive_file(b"x.mkv"));
        assert!(is_known_collection_ext(b".mkv"));
        assert!(is_known_collection_ext(b".par2"));
        assert!(!is_known_collection_ext(b""));
        assert!(!is_known_collection_ext(b".xyz123"));
    }

    #[test]
    fn sniff() {
        assert_eq!(sniff_extension(b"Rar!\x1a\x07\x01\x00"), c".rar");
        assert_eq!(sniff_extension(b"\x1a\x45\xdf\xa3....webm"), c".webm");
        assert_eq!(sniff_extension(b"...."), c"");
    }

    #[test]
    fn filetypes_full_suite_port_from_cpp() {
        // FileTypesRarFileTest
        assert!(is_rar_file(b"archive.rar"));
        assert!(is_rar_file(b"ARCHIVE.RAR"));
        assert!(is_rar_file(b"archive.part01.rar"));
        assert!(is_rar_file(b"archive.r00"));
        assert!(is_rar_file(b"archive.r99"));
        assert!(is_rar_file(b"archive.s01"));
        assert!(is_rar_file(b"archive.z50"));
        assert!(is_rar_file(b"archive.rar.001"));
        assert!(is_rar_file(b"/path/to/archive.rar.012"));
        assert!(is_rar_file(b"C:\\path\\to\\archive.r00"));

        assert!(!is_rar_file(b""));
        assert!(!is_rar_file(b"noext"));
        assert!(!is_rar_file(b"archive.7z.001"));
        assert!(!is_rar_file(b"archive.001"));
        assert!(!is_rar_file(b"archive.q00"));
        assert!(!is_rar_file(b"archive.r0a"));
        assert!(!is_rar_file(b"archive.r000"));
        assert!(!is_rar_file(b"video.mkv"));

        // FileTypesSevenZipFileTest
        assert!(is_seven_zip_file(b"archive.7z"));
        assert!(is_seven_zip_file(b"ARCHIVE.ZIP"));
        assert!(is_seven_zip_file(b"archive.tar"));
        assert!(is_seven_zip_file(b"archive.gz"));
        assert!(is_seven_zip_file(b"archive.bz"));
        assert!(is_seven_zip_file(b"archive.bz2"));
        assert!(is_seven_zip_file(b"archive.tgz"));
        assert!(is_seven_zip_file(b"archive.txz"));
        assert!(is_seven_zip_file(b"archive.xz"));
        assert!(is_seven_zip_file(b"archive.7z.001"));
        assert!(is_seven_zip_file(b"archive.zip.001"));
        assert!(is_seven_zip_file(b"archive.tar.gz"));
        assert!(is_seven_zip_file(b"archive.tar.bz2"));
        assert!(is_seven_zip_file(b"archive.tar.xz"));

        assert!(!is_seven_zip_file(b""));
        assert!(!is_seven_zip_file(b"noext"));
        assert!(!is_seven_zip_file(b"archive.7z.002"));
        assert!(!is_seven_zip_file(b"archive.001"));
        assert!(!is_seven_zip_file(b"archive.rar"));
        assert!(!is_seven_zip_file(b"archive.rar.001"));
        assert!(!is_seven_zip_file(b"video.mkv"));

        // FileTypesArchiveFileTest
        assert!(is_archive_file(b"archive.rar"));
        assert!(is_archive_file(b"archive.r01"));
        assert!(is_archive_file(b"archive.7z"));
        assert!(is_archive_file(b"archive.7z.001"));
        assert!(is_archive_file(b"archive.rar.001"));
        assert!(!is_archive_file(b"archive.001"));
        assert!(!is_archive_file(b"video.mkv"));

        assert!(is_archive_ext(b".rar"));
        assert!(is_archive_ext(b".r00"));
        assert!(is_archive_ext(b".7z"));
        assert!(is_archive_ext(b".001"));
        assert!(!is_archive_ext(b".0001"));
        assert!(is_all_digits_ext(b".0001"));
        assert!(!is_all_digits_ext(b".00a"));
        assert!(!is_all_digits_ext(b"001"));

        // FileTypesSampleTest
        assert!(is_sample_stem(b"sample"));
        assert!(is_sample_stem(b"SAMPLE"));
        assert!(is_sample_stem(b"movie-sample"));
        assert!(is_sample_stem(b"movie.sample"));
        assert!(is_sample_stem(b"movie_sample"));
        assert!(is_sample_stem(b"Movie.2024.1080p-SAMPLE"));

        assert!(!is_sample_stem(b""));
        assert!(!is_sample_stem(b"sampler"));
        assert!(!is_sample_stem(b"samples"));
        assert!(!is_sample_stem(b"sample-movie"));
        assert!(!is_sample_stem(b"movie.sample.extra"));

        assert!(is_sample_file(b"sample.mkv"));
        assert!(is_sample_file(b"/downloads/Movie/movie-sample.mkv"));
        assert!(is_sample_file(b"C:\\downloads\\Movie\\movie_sample.mp4"));
        assert!(!is_sample_file(b"Movie.2024.1080p.mkv"));
        assert!(!is_sample_file(b"SampleMovie.mkv"));

        // FileTypesDiscAndClutterTest
        assert!(is_disc_structure_ext(b".vob"));
        assert!(is_disc_structure_ext(b".M2TS"));
        assert!(is_disc_structure_ext(b".bdmv"));
        assert!(is_disc_structure_ext(b".ifo"));
        assert!(!is_disc_structure_ext(b".mkv"));

        assert!(is_disc_structure_dir(b"BDMV"));
        assert!(is_disc_structure_dir(b"/path/to/VIDEO_TS"));
        assert!(is_disc_structure_dir(b"C:\\disc\\avchd"));
        assert!(!is_disc_structure_dir(b"Movies"));

        assert!(is_disc_descriptor_ext(b".cue"));
        assert!(is_disc_descriptor_ext(b".MDS"));
        assert!(is_disc_image_ext(b".iso"));
        assert!(is_disc_image_ext(b".mdf"));
        assert!(is_generic_disc_image_ext(b".bin"));
        assert!(is_generic_disc_image_ext(b".IMG"));

        assert!(is_clutter_dir(b"@eaDir"));
        assert!(is_clutter_dir(b"/volume1/video/__MACOSX"));
        assert!(is_clutter_dir(b".Spotlight-V100"));
        assert!(!is_clutter_dir(b"Subs"));

        assert!(is_clutter_file(b".DS_Store"));
        assert!(is_clutter_file(b"Thumbs.db"));
        assert!(is_clutter_file(b"desktop.ini"));
        assert!(is_clutter_file(b"._Movie.mkv"));
        assert!(is_clutter_file(b"/path/to/._Movie.mkv"));
        assert!(!is_clutter_file(b"._"));
        assert!(!is_clutter_file(b".hidden.mkv"));
        assert!(!is_clutter_file(b"Movie.mkv"));

        // FileTypesMediaExtensionsTest
        assert!(is_parity_ext(b".par2"));
        assert!(is_parity_ext(b".PAR2"));
        assert!(is_parity_ext(b".sfv"));
        assert!(!is_parity_ext(b".par"));

        assert!(is_video_ext(b".mkv"));
        assert!(is_video_ext(b".MP4"));
        assert!(!is_video_ext(b".mp3"));

        assert!(is_audio_ext(b".flac"));
        assert!(is_audio_ext(b".MP3"));
        assert!(!is_audio_ext(b".mkv"));

        assert!(is_subtitle_ext(b".srt"));
        assert!(is_subtitle_ext(b".SUP"));
        assert!(is_nfo_ext(b".nfo"));
        assert!(is_nfo_ext(b".INFO"));
        assert!(is_book_ext(b".epub"));
        assert!(is_book_ext(b".PDF"));
        assert!(is_image_ext(b".jpg"));
        assert!(is_image_ext(b".PNG"));

        // FileTypesKnownCollectionExtensionTest
        assert!(is_known_collection_ext(b".mkv"));
        assert!(is_known_collection_ext(b".flac"));
        assert!(is_known_collection_ext(b".srt"));
        assert!(is_known_collection_ext(b".nfo"));
        assert!(is_known_collection_ext(b".epub"));
        assert!(is_known_collection_ext(b".jpg"));
        assert!(is_known_collection_ext(b".rar"));
        assert!(is_known_collection_ext(b".r00"));
        assert!(is_known_collection_ext(b".001"));
        assert!(is_known_collection_ext(b".par2"));
        assert!(is_known_collection_ext(b".vob"));
        assert!(is_known_collection_ext(b".iso"));
        assert!(is_known_collection_ext(b".cue"));
        assert!(is_known_collection_ext(b".bin"));

        assert!(!is_known_collection_ext(b""));
        assert!(!is_known_collection_ext(b"mkv"));
        assert!(!is_known_collection_ext(b".0001"));
        assert!(!is_known_collection_ext(b".a1b2"));

        // FileTypesSniffExtensionHeaderTest
        assert_eq!(sniff_extension(b"%PDF-1.7\n"), c".pdf");
        assert_eq!(sniff_extension(b"\x1a\x45\xdf\xa3\x93\x42\x82\x88matroska"), c".mkv");
        assert_eq!(sniff_extension(b"\x1a\x45\xdf\xa3\x93\x42\x82\x84webm"), c".webm");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00"), c".mp4");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x18ftypM4V \x00\x00\x00\x00"), c".m4v");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00"), c".m4a");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x18ftypM4B \x00\x00\x00\x00"), c".m4b");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x14ftypqt  \x20\x05\x03\x00"), c".mov");
        assert_eq!(sniff_extension(b"\x00\x00\x00\x1cmoovlmvhd"), c".mp4");
        assert_eq!(sniff_extension(b"RIFF\x24\x00\x00\x00AVI LIST"), c".avi");
        assert_eq!(sniff_extension(b"RIFF\x24\x00\x00\x00WAVEfmt "), c".wav");
        assert_eq!(sniff_extension(b"RIFF\x24\x00\x00\x00WEBPVP8 "), c".webp");
        assert_eq!(sniff_extension(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"), c".png");
        assert_eq!(sniff_extension(b"GIF89a\x01\x00\x01\x00"), c".gif");
        assert_eq!(sniff_extension(b"\xff\xd8\xff\xe0\x00\x10JFIF"), c".jpg");
        assert_eq!(
            sniff_extension(b"BM\x36\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00"),
            c".bmp"
        );
        assert_eq!(sniff_extension(b"fLaC\x00\x00\x00\x22"), c".flac");
        assert_eq!(sniff_extension(b"ID3\x04\x00\x00\x00\x00\x00\x00"), c".mp3");
        assert_eq!(sniff_extension(b"\xff\xfb\x90\x64\x00\x00"), c".mp3");
        assert_eq!(sniff_extension(b"OggS\x00\x02\x00\x00"), c".ogg");
        assert_eq!(
            sniff_extension(b"PK\x03\x04\x14\x00\x00\x00\x08\x00file.txt"),
            c".zip"
        );
        assert_eq!(
            sniff_extension(
                b"PK\x03\x04\x14\x00\x00\x00\x00\x00mimetypeapplication/epub+zip"
            ),
            c".epub"
        );
        assert_eq!(sniff_extension(b"AT&TFORM\x00\x00\x01\x00DJVUINFO"), c".djvu");
        assert_eq!(sniff_extension(b"Rar!\x1a\x07\x00\xcf\x90"), c".rar");
        assert_eq!(sniff_extension(b"7z\xbc\xaf\x27\x1c\x00\x04"), c".7z");
        assert_eq!(sniff_extension(b"\x1f\x8b\x08\x00\x00\x00\x00\x00"), c".gz");
        assert_eq!(sniff_extension(b"BZh91AY&SY"), c".bz2");
        assert_eq!(sniff_extension(b"\xfd7zXZ\x00\x00\x04"), c".xz");
        assert_eq!(sniff_extension(b"\x1f\x9d\x90"), c".Z");

        let mut ts = vec![0u8; 377];
        ts[0] = 0x47;
        ts[188] = 0x47;
        ts[376] = 0x47;
        assert_eq!(sniff_extension(&ts), c".ts");

        let mut mobi = vec![0u8; 68];
        mobi[60..68].copy_from_slice(b"BOOKMOBI");
        assert_eq!(sniff_extension(&mobi), c".mobi");

        let mut tar = vec![0u8; 263];
        tar[257..262].copy_from_slice(b"ustar");
        assert_eq!(sniff_extension(&tar), c".tar");

        let asf_hdr = b"\x30\x26\xb2\x75\x8e\x66\xcf\x11\xa6\xd9\x00\xaa\x00\x62\xce\x6c";
        let asf_vid = b"\xc0\xef\x19\xbc\x4d\x5b\xcf\x11\xa8\xfd\x00\x80\x5f\x5c\x44\x2b";
        let asf_aud = b"\x40\x9e\x69\xf8\x4d\x5b\xcf\x11\xa8\xfd\x00\x80\x5f\x5c\x44\x2b";
        assert_eq!(sniff_extension(asf_hdr), c"");
        let mut wmv = asf_hdr.to_vec();
        wmv.extend_from_slice(asf_vid);
        assert_eq!(sniff_extension(&wmv), c".wmv");
        let mut wma = asf_hdr.to_vec();
        wma.extend_from_slice(asf_aud);
        assert_eq!(sniff_extension(&wma), c".wma");

        assert_eq!(sniff_extension(b""), c"");
        assert_eq!(sniff_extension(b"plain text file contents"), c"");

        // FileTypesSniffExtensionFileTest
        let tmp_path = std::env::temp_dir().join(format!(
            "nzbget-filetypes-sniff-rust-{}.bin",
            std::process::id()
        ));
        std::fs::write(&tmp_path, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").unwrap();
        assert_eq!(sniff_extension_file(&tmp_path), c".png");
        let _ = std::fs::remove_file(&tmp_path);
        assert_eq!(sniff_extension_file(&tmp_path), c"");
    }
}
