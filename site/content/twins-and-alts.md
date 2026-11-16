+++
title = "Twins and alts"
description = "What a twin and an alt are, why RustyGet compares the video inside a copy instead of its archive bytes, which fingerprints can link two Usenet posts cheaply, and why a checksum alone is never proof."
eyebrow = "Features"
lede = "A release is often posted many times. Some copies are the exact same post uploaded again; others hold the same video packed another way. RustyGet tells the two apart, because each kind can repair the primary in a different way."
+++

> [!NOTE] Where this page fits
> [Duplicate fleets](duplicate-fleets.md) explains what RustyGet does with a fleet: borrowing, stream repair, and failover. This page explains the idea underneath: how two copies are linked, and what each link allows.

## Two words, one meaning each

Twin
: The exact same post, uploaded again: the same files, the same article counts, and the same checksums. Because every article lines up with an article of the primary, a twin's articles can be swapped in one for one. Its par2 files can also repair the primary.

Alt
: Any container that holds the same video file, the same `.mkv`, packed another way: other volume sizes, RAR instead of 7-Zip, an archive inside an archive, or another password. An alt is either a **borrowable alt**, whose video bytes RustyGet can read by range without unpacking, or a **non-borrowable alt**, which has to be downloaded and extracted whole first, or, without its password, serves only as a failover backup.

A copy with a different video file, such as another encode, is neither. It's another release that happens to share a duplicate key.

## Compare the video, never the container

Two alts never share container bytes. A new volume size moves every byte; a new password, or even a new file time, changes the archive headers. Comparing those bytes would say two copies differ when the movie inside is identical.

So RustyGet compares and borrows on the **inner payload**: the bytes of the final video file. It reaches them through every stored layer around them, such as a 7-Zip archive in copy mode, a RAR4 or RAR5 archive in store mode, an archive inside an archive, or an encrypted RAR or 7-Zip archive whose password it knows, and builds a map of where each video byte sits inside each copy. A missing range in one copy is then a known range in another.

The same rule works in reverse. When two copies have different par2 checksums, that proves only that their containers differ. It never means "another encode": the video inside can still be the same, and only reading the video can tell.

## Fingerprints a post carries

Every post leaves fingerprints that can link it to another post without downloading the whole thing. They differ in cost and in what they can prove.

| Fingerprint | Where it comes from | Cost | What a match suggests |
|---|---|---|---|
| Sizes in the NZB file | The NZB file itself | Free | A filter only. Many unrelated files share a size, and every encrypted repost of one release shares the sizes of the others. |
| Part checksums | The checksum at the end of each yEnc article (`pcrc32`, or `crc32` for a single-part post), when the poster includes one | Already there for every article that downloads | Same bytes in that part. The parts of a file combine into a checksum of the whole file. |
| par2 file list | One par2 index article | One article | Each file's MD5 and length. Equal lists mean the files are byte-identical, whatever their names. |
| Archive headers | The first article of each RAR volume or of a ZIP's first piece; for 7-Zip, the end of the last volume, where it keeps its headers | One or two articles | The name and size of the file packed inside, and how it's packed: whether it's stored or compressed, and whether it's encrypted. The checksum covers the whole file only in the last volume of a split RAR; earlier volumes carry the checksum of their own part. |
| Sampled video bytes | A few places spread across the video, read through the containers | A few articles | The same video inside, whatever the packaging. |
| Encryption salts | The header of an encrypted archive | One article | The same encryption run. Each run picks a fresh random salt (RAR) or initialization vector (7-Zip), so two separately encrypted archives never share one. |

The cheap fingerprints narrow the field; the stronger ones confirm. No single key should decide a link on its own, because every key has a known way to match wrongly.

## Why a checksum alone isn't proof

yEnc checksums, and the file checksums RAR stores by default, are CRC32 values, only 32 bits long. Among millions of files, two different files share one by chance, and an equal size adds little, because many unrelated files share round sizes. A CRC32 can also be forced to any value by changing four bytes. So a matching CRC32 makes a good candidate and a poor verdict.

Sizes are weaker still. A re-upload with a new password has exactly the same volume sizes as the first upload, because the size of an encrypted archive doesn't depend on the password. Size, file count, and total bytes never make a twin on their own.

What can make a twin: a full MD5 together with the length, as par2 lists them. Matching articles at the same positions are weaker evidence, and only strong when they include the articles that carry archive headers, because that's where a re-pack differs.

## Re-packs and encrypted reposts

Re-pack
: An archive packed again with the same files, often with only a new file time. Stored archives are deterministic, so most of the articles in the middle come out byte-identical; only the articles holding headers differ. par2 lists one MD5 per file, though, and in a multi-volume RAR every volume carries a header, so every volume's MD5 changes. Such a copy shares most of its par2 file list with the primary only when the changed headers sit in a few of its files, as in a 7-Zip split into `.001` pieces or a release of many separate files. RustyGet labels it `TWIN N/M` when more than half of the files match.

Re-RAR
: The same file packed again as RAR with a new file time. Only the first article and the last few articles of each volume differ, where the headers, the end header, and any recovery record sit. RustyGet proves such a copy an alt and lends its middle articles one for one, never a header article. This works only when the post is exactly one set of RAR volumes holding one file that runs from the first volume into the last, because packing again rewrites every file header. A post with an NFO or SFV file beside the volumes, or with several files inside, is a plain alt.

Encrypted repost
: An upload whose archive headers are encrypted, such as RAR5 with `-hp` or 7-Zip with encrypted headers. Encryption hides every content fingerprint. An archive encrypted again gets a fresh salt or initialization vector that changes every byte, so it's never a twin of another encryption run; the same encrypted files uploaded again are still a twin. With the password in the NZB file, stream repair can still read through a stored, encrypted copy, so it can repair the primary, at a cost. Without the password, it can only be a failover backup.

## What RustyGet checks today

The preceding table lists what a post carries. RustyGet uses some of those fingerprints now:

- **Twins** are proven by par2: both copies' par2 file lists must name the same files by MD5 and length, or more than half of them for a near-twin (`TWIN N/M`). To rank as a twin in a fleet, their data files must also split into the same numbers of articles. Without par2, TwinCheck compares the first and last article of up to three files, where the archive headers sit, plus six articles from the middle. Matching header articles make a twin; matching middle articles with different header articles make a re-RAR or a plain alt. A copy without par2 still never ranks as a twin in a fleet.
- **Encryption salts** settle it early: RustyGet reads the salt and initialization vector from the first article of an encrypted RAR, and different values mean another archiver run, so never a twin.
- **Different article sizes** are compared where both postings start an article at the same offset. Identical bytes there make an alt that holds the same files, but its articles are never lent one for one.
- **Alts** are proven at fleet intake. RustyGet opens every live copy down to its video and reads its start plus 2, 4, or 8 random places, the same places in every copy. Copies that read the same are the same video. A copy holding another video is dropped from the fleet, and a copy that can't be opened stays as an unverified backup. A 7-Zip with a password but without encrypted headers still shows the size and CRC32 of the file inside; when the video can't be read, matching values on a file of 50 MiB or more confirm the alt together with the other checks, never on their own.
- **Late backups** sent in a later fleet are checked against the copies of the key already queued or kept as backups, before they download. A fleet of a single copy gets a kind too.
- **How borrowable** an alt is comes from the first article of its largest file. A bare video, a raw split, and a stored RAR or ZIP are borrowable. A stored RAR whose data is encrypted with a password in the NZB file can be read, at a cost, so it ranks after them. A 7-Zip keeps its headers at its end, so its first article can't tell: without a password it ranks in the middle, and with a password it ranks last. That holds for a 7-Zip split into pieces too. A ZIP entry that's encrypted ranks last even with the password, because stream repair doesn't decrypt ZIP. Compression, encryption without a known password, and RAR archives with encrypted headers rank last too, even with the password, because nothing about their content can be read from that first article.
- **Borrowed data** is checked before it counts: against bytes the primary already has, against yEnc checksums where the article carries one, and against the primary's par2 blocks where its par2 set covers them. Stream repair proves a donor with probe reads, and the par check after the download has the final say. See [Making sure the bytes are right](duplicate-fleets.md#making-sure-the-bytes-are-right).

Part checksums are checked for each article as it downloads, but RustyGet doesn't combine them into whole-file keys or link copies on them yet. Linking on the checksum in a RAR's last volume or on the sizes of encrypted reposts isn't built either.

## Donor order

Copies are ranked by kind:

1. Twins. Their articles fill holes directly.
2. Borrowable alts. Their video bytes fill holes through the map.
3. Copies whose packaging couldn't be told, such as a 7-Zip without a password, and stored RAR copies whose data needs the password to decrypt.
4. Non-borrowable alts. They're downloaded and extracted whole, or serve as failover backups.

A fleet puts health first: healthy copies, with at least 97.5% of the checked articles alive, come before weak ones, then copies of unknown health, then dead ones. The kinds order the copies within each of those groups, so a copy that's missing many articles never leads however cheap its packaging. RustyGet sets each copy's duplicate score itself, so a client can't push an encrypted copy ahead of a borrowable one. Scores start in that rank order; once a download runs, the backup check of `DupeWatch` scores the backups again in the same order, and a failing download is swapped for a backup in that order too.

Article fallback asks twins first, healthiest first, then the other copies by kind, then by duplicate score. A copy added outside a fleet has no kind from intake: article fallback counts it as a twin when par2 proved one, otherwise by the kind the `DupeWatch` backup check worked out for it, and with the copies whose packaging couldn't be told when it has none. Stream repair asks donors by duplicate score alone.

A single copy is used whatever its kind.
