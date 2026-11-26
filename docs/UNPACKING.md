## Unpacking

RustyGet unpacks archives itself, in-process, with the [zipfast](https://github.com/xbmc4lyfe/zipfast) library: RAR (1.5 through 5.0, multi-volume, encrypted data and headers), 7-Zip (including split `.7z.001`), ZIP, TAR, Gzip, Bzip2, XZ and Zstandard. There are no `unrar` or `7z` programs to install or configure.

- **Direct unpack** extracts while the download arrives, so the output is current as soon as the bytes land:
  - RAR sets are read from their first volume on, as the download goes.
  - Byte-split 7z sets (`.7z.001`), byte-split ZIP sets (`.zip.001`) and Info-ZIP split sets (`.z01` ... `.zip`) are extracted live too. Their directory sits in the last volume, so that volume is downloaded first; every file inside is then written straight into its output as the volumes arrive.
  - With `DirectWrite` (the default), a volume that is still downloading is read article by article: each article is decoded as soon as it is written into place, not after its volume has finished. When the last article of a set lands, nearly everything before it is already extracted, and a volume the extraction reached while it was downloading is never read back once finished. Articles held in the article cache are written out at once while the extraction waits for them. Without `DirectWrite`, volumes are read as each one finishes.
  - Split files (`movie.mkv.001`, `.002` ...) are joined as their pieces arrive: each piece is placed at its offset in `movie.mkv`, and nothing is joined after the download.
  - A set direct unpack cannot read live (a 7z method it cannot stream, an archive inside the archive, encrypted 7z headers without the password) is left to post-processing, which extracts it as before.
- **Damaged volumes** do not stop direct unpack where the archive allows it. Each volume's own headers say where every file's data lies in it: a lost range inside that data is read past and filled in once the volume is repaired (from a duplicate or par2), wherever it lies in the volume. A RAR5 volume that lost its end (the records behind its data) gets it rebuilt byte for byte from its own headers and length as it finishes. Only a lost header in front of data waits for the repair.
- **Encrypted archives** use the password from the NZB or the download's `*Password*` setting.
- **Nested archives**, obfuscated archive names and file permissions are handled in the same pipeline.

See [Check, repair, and unpack](https://rustyget.xbmc4lyfe.com/post-processing.html) for the settings.
