# twinfields fixtures

Real archives made with rar 7.00 (Ubuntu 24.04 `rar` package) and 7-Zip 23.01,
all packing the same file: `video.mkv`, 49156 bytes (`1a45dfa3` then 49152
bytes from Python `random.Random(20261116).getrandbits(8)`), CRC32 `771fbd29`.
The video itself is not kept.

| Files | Made with |
| --- | --- |
| `rerar-a.part{1,2,3}.rar` | `rar a -m0 -v20k -ma5`, mtime 2024-01-01 10:00:00 |
| `rerar-c.part{1,2,3}.rar` | the same, mtime 10:02:03 (+123 s): a re-RAR of `rerar-a` |
| `hp1.part{1,2}.head`, `hp2.part1.head` | first 4096 bytes of two `rar a -m0 -v20k -ma5 -hpsecret` runs |
| `p1.part1.head`, `p2.part1.head` | first 4096 bytes of two `rar a -m0 -v20k -ma5 -psecret` runs |
| `plain.7z` | `7z a -mx0` |
| `pass.7z` | `7z a -mx0 -psecret` (headers in the clear) |
| `pass-mhe.7z` | `7z a -mx0 -psecret -mhe=on` |
| `pass-split.7z.00{1,2,3}` | `7z a -mx0 -psecret -v20k` |

rar 7.00 cannot write RAR4 (`-ma4` was removed), so RAR4 salts are covered by
zipfast's own tests only.
