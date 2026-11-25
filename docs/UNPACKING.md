## Unpacking

RustyGet unpacks archives itself, in-process, with the [zipfast](https://github.com/xbmc4lyfe/zipfast) library: RAR (1.5 through 5.0, multi-volume, encrypted data and headers), 7-Zip (including split `.7z.001`), ZIP, TAR, Gzip, Bzip2, XZ and Zstandard. There are no `unrar` or `7z` programs to install or configure.

- **Direct unpack** extracts while the download arrives, so the output is current as soon as the bytes land.
- **Encrypted archives** use the password from the NZB or the download's `*Password*` setting.
- **Nested archives**, obfuscated archive names and file permissions are handled in the same pipeline.

See [Check, repair, and unpack](https://rustyget.xbmc4lyfe.com/post-processing.html) for the settings.
