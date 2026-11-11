// C ABI of the Rust parts of nzbget (rust/src/ffi.rs)
#ifndef NZBGET_RS_H
#define NZBGET_RS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct
{
	char* data; // Rust-owned, NUL-terminated; never pass to free()/realloc()
	size_t len;
	size_t cap;
} NzbgetRsBuf;

// Twins and alts (rust/src/twin.rs). Lists are text lines: a file list is
// "length\tmd5\tname" per file. A download is given by its id and its
// identity "fingerprint#posting" (DupeFiles and the posting's message-id
// hash): a stored list is used only for the posting it was made for.
void nzbget_rs_twin_open(const char* path);
// returns "fingerprint\tcomplete" (complete: 1 or 0)
NzbgetRsBuf nzbget_rs_twin_put_par2(int id, const char* posting, const unsigned char* data, size_t len);
// 0: no list stored for this fingerprint, 1: a partial one, 2: a complete one
int nzbget_rs_twin_has(int id, const char* print);
// keeps the lists of the postings given as "id\tfingerprint" lines
void nzbget_rs_twin_prune(const char* lines);
// The fingerprint of count files given as md5 texts (NULL entries empty) and
// lengths; their names are not part of it.
NzbgetRsBuf nzbget_rs_twin_fingerprint(const char* const* md5s, const unsigned long long* lengths, size_t count);
unsigned long long nzbget_rs_par2_block_size(const unsigned char* data, size_t len);
int nzbget_rs_par2_volume_blocks(const char* filename);
NzbgetRsBuf nzbget_rs_twin_kind(int primary, const char* primaryPrint, int dupe, const char* dupePrint);
NzbgetRsBuf nzbget_rs_twin_match(int own, const char* ownPrint, const char* target, const char* targetAlt,
	int donor, const char* donorPrint);
// Release each result exactly once, with all fields unchanged. A zero buffer
// is also accepted. Copy the bytes before freeing if they must outlive the result.
void nzbget_rs_free(NzbgetRsBuf buf);

// Util's string helpers (rust/src/util.rs). Format results are Rust-owned;
// copy and release with nzbget_rs_free. Numeric formatting uses LC_NUMERIC.
// Panics abort and never unwind through C++.
NzbgetRsBuf nzbget_rs_format_size(long long size);
// NULL strings are empty; non-NULL strings must be readable and NUL-terminated.
int nzbget_rs_match_file_ext(const char* filename, const char* extensionList, const char* listSeparator);

// FeedFilter (rust/src/feedfilter.rs): the feed item stays in C++ and is read
// and changed through these callbacks, in the C++ order. Fields: title,
// filename, category, url, size, age, imdbid, rageid, tvdbid, tvmazeid,
// description, season, episode, priority, dupekey, dupescore, dupestatus,
// attr- (0..17). Match status: 0 ignored, 1 accepted, 2 rejected.
// Callbacks must not throw/unwind or reenter/free the same filter. Callback
// strings are borrowed: returned strings stay valid until the next callback;
// strings passed to callbacks are valid only for that call and must be copied
// if retained. regexNew handles must stay valid for this filter's lifetime.
typedef struct NzbgetRsFeedOptions
{
	int hasPause, pause;
	int hasCategory; const char* category;
	int hasPriority, priority;
	int hasAddPriority, addPriority;
	int hasDupeScore, dupeScore;
	int hasAddDupeScore, addDupeScore;
	int hasBuildDupeKey; const char* ids[4]; // rageid, tvdbid, tvmazeid, series
	int hasDupeKey; const char* dupeKey;
	int hasAddDupeKey; const char* addDupeKey;
	int hasDupeMode, dupeMode;
} NzbgetRsFeedOptions;
typedef struct NzbgetRsFeedItem
{
	void* user;
	void (*field)(void* user, int field, const char* attr, const char** str, long long* num);
	const char* (*seasonEpisode)(void* user, int episode);
	void (*apply)(void* user, const NzbgetRsFeedOptions* options);
	void (*setMatch)(void* user, int status, int rule);
} NzbgetRsFeedItem;
typedef struct NzbgetRsFeedFilter NzbgetRsFeedFilter;
// Copies filter (NULL means empty). The opaque handle is owned by Rust and
// must only be released with feed_filter_free; free(NULL) is harmless.
NzbgetRsFeedFilter* nzbget_rs_feed_filter_new(const char* filter);
void nzbget_rs_feed_filter_free(NzbgetRsFeedFilter* filter);
// NULL filter/item or missing callbacks are a no-op. All callbacks are
// required. Matching requires exclusive access to the filter and its regex handles.
void nzbget_rs_feed_filter_match(NzbgetRsFeedFilter* filter, const NzbgetRsFeedItem* item);

// Deobfuscation (rust/src/deobfuscation.rs): text as pointer and length.
// NULL means empty regardless of len; otherwise len bytes must be readable.
int nzbget_rs_is_excessively_obfuscated(const char* str, size_t len);

// FileTypes (rust/src/filetypes.rs): name checks by number, in FileTypes.h's
// order (IsSevenZipExt 0 ... IsKnownCollectionExt 25). Inputs are borrowed byte spans;
// NULL means empty regardless of len. Panics abort at the ABI boundary.
int nzbget_rs_file_type(int which, const char* str, size_t len);

// FileSystem's path texts (rust/src/paths.rs): op 0 MakeValidFilename (flag:
// allow slashes), 1 SanitizePathSegment, 2 SanitizeRelativePath,
// 3 EscapePathForShell. Positions: op 0 BaseFileName's start, 1 the last
// '/' or '\\' (SIZE_MAX for none), 2 ExtractFilePathFromCmd's length.
// str is NULL (empty regardless of len) or readable for len bytes in one
// allocation, with len <= PTRDIFF_MAX. No input terminator is required.
// Text results own their storage; release exactly once with nzbget_rs_free.
// Panics/allocation failures abort; Rust never unwinds across this ABI.
NzbgetRsBuf nzbget_rs_path_text(int op, const char* str, size_t len, int flag);
size_t nzbget_rs_path_position(int op, const char* str, size_t len);
// path is NULL (no-op) or a writable NUL-terminated string, owned by the caller.
void nzbget_rs_normalize_path_separators(char* path);

// Decoder (rust/src/decoder.rs), with rapidyenc's incremental decoder (its
// state as an int) and CRC. Get: 0 format, 1 begin, 2 end, 3 size,
// 4 expected CRC, 5 calculated CRC, 6 EOF. Set: 0 CRC check, 1 raw mode.
// Handles are Rust-owned, freed once with decoder_free, never used concurrently.
// NULL callbacks make new return NULL; NULL handles/inputs are accepted as no-ops
// (check returns UnknownError, getters return zero/an empty borrowed string).
// decode: buffer is caller-owned and disjoint from the decoder. It must fit the
// input AND output (up to input length + 63 for a buffered UU line; Connection
// reserves 128). In line mode len==0 means strlen(buffer), as in StringBuilder.
// Negative lengths are ignored. Callbacks must not unwind; Rust panics abort.
// filename is borrowed until the decoder changes; do not free it.
typedef struct NzbgetRsDecoder NzbgetRsDecoder;
typedef int (*NzbgetRsYencDecode)(const void** src, void** dst, size_t len, int* state);
typedef unsigned int (*NzbgetRsCrc)(const void* src, size_t len, unsigned int init);
NzbgetRsDecoder* nzbget_rs_decoder_new(NzbgetRsYencDecode decode, NzbgetRsCrc crc);
void nzbget_rs_decoder_free(NzbgetRsDecoder* decoder);
void nzbget_rs_decoder_clear(NzbgetRsDecoder* decoder);
int nzbget_rs_decoder_decode(NzbgetRsDecoder* decoder, char* buffer, int len);
int nzbget_rs_decoder_check(NzbgetRsDecoder* decoder);
void nzbget_rs_decoder_set(NzbgetRsDecoder* decoder, int which, int value);
long long nzbget_rs_decoder_get(NzbgetRsDecoder* decoder, int which);
const char* nzbget_rs_decoder_filename(NzbgetRsDecoder* decoder);

// Scheduler::CheckTasks' timing (rust/src/scheduler.rs): which tasks are due
// between *lastCheck and current. Local times use separate offset readings:
// current + currentOffset and *lastCheck + lastCheckOffset, as in C++. Updates the
// tasks' lastExecuted and *lastCheck, writes the due task indexes in execution
// order to due (room for dueCapacity) and returns how many; *reset tells whether
// the clock jumped (> 90 minutes or back) and a week was rechecked.
// If the return value exceeds dueCapacity, no outputs change; retry with that
// many entries. count * 9 is a usual capacity, not a bound for every libc TZif.
// All buffers are disjoint and caller-owned. NULL lastCheck/reset, or NULL
// tasks with nonzero count, or NULL due with nonzero dueCapacity, returns 0
// without changing outputs. With zero count/capacity, tasks/due may be NULL.
// Unrepresentable buffer sizes are also rejected.
typedef struct NzbgetRsSchedTask
{
	int hours; // -1: startup task
	int minutes;
	int weekDays; // bit n: weekday n + 1 (1 Monday .. 7 Sunday); 0: all
	long long lastExecuted;
} NzbgetRsSchedTask;
// Calendar conversion stays in libc: leap-aware TZif files affect gmtime_r.
// year is the full year; mon is 0..11 and wday is 0 (Sunday)..6.
typedef struct NzbgetRsSchedTm
{
	long long year, mon, mday, hour, min, sec, wday;
} NzbgetRsSchedTm;
// gmtime must fill all fields and must not throw. NULL is rejected atomically.
size_t nzbget_rs_scheduler_check(NzbgetRsSchedTask* tasks, size_t count, long long* lastCheck, long long current, long long currentOffset, long long lastCheckOffset, size_t* due, size_t dueCapacity, int* reset, void (*gmtime)(long long, NzbgetRsSchedTm*));

// Util text helpers (rust/src/util.rs). split_command_line: the words, each
// NUL-terminated, back to back (len covers all). trim_line: TrimRight(char*)
// (rightOnly) or Trim(char*) in place, returning the start. trim_string:
// TrimLeft/TrimRight/Trim of a std::string, or SanitizeLine (which blanks
// control characters in place): the kept range [*start, result). ends_with:
// EndsWith on byte ranges. parse_rfc822_date_time: WebUtil's, 0 if invalid.
// NULL inputs are empty; trim_line(NULL) returns NULL. trim_string accepts a
// NULL start output; rightSpace classifies unsigned bytes as the caller's char
// in its current C locale (NULL means no right whitespace). The callback must
// not throw or access the buffer. All input buffers remain caller-owned.
// Rust panics abort and never unwind across the ABI.
// Free returned buffers with nzbget_rs_free.
NzbgetRsBuf nzbget_rs_split_command_line(const char* s);

// ServerVolume's slots (rust/src/statmeter.rs). calc_slots: CalcSlots for a
// local time (cut to an int as before), updating *firstDay; day is -1 outside
// the 20 years from 2013, inRange tells whether the day arrays may grow.
// volume_add: AddStats' clearing of the second/minute/hour slots passed since
// locDataTime, then adding bytes at the slots; slots outside an array (a time
// before 1970 or after 2038-01-19 as an int) are skipped.
// Arrays stay caller-owned, must be disjoint and writable for their lengths
// (in int64_t elements). NULL arrays are ignored; NULL slots makes add a no-op.
// The slots descriptor is copied before accessing arrays and may overlap one.
// calc_slots is a no-op if either output pointer is NULL.
typedef struct NzbgetRsVolumeSlots
{
	int sec, min, hour, day, inRange;
} NzbgetRsVolumeSlots;
void nzbget_rs_volume_calc_slots(long long locCurTime, int* firstDay, NzbgetRsVolumeSlots* slots);
void nzbget_rs_volume_add(int64_t* seconds, size_t secondsLen, int64_t* minutes, size_t minutesLen,
	int64_t* hours, size_t hoursLen, const NzbgetRsVolumeSlots* slots, int lastMinSlot, int lastHourSlot,
	long long locCurTime, long long locDataTime, int64_t bytes);

// Options' value parsers (rust/src/options.rs). parse_time/parse_week_days
// write their outputs as the C++ did, also on failure. validate_option_name:
// 0 invalid, 1 valid, 2 obsolete (warn), 3 obsolete script option (error with
// a value), 4 obsolete log option; predefined(ctx, name) is GetOption's answer.
// convert_old_option: the current name and value (free with nzbget_rs_free).
// NULL inputs mean empty strings; NULL outputs discard the corresponding result.
// Identical output pointers retain only the new value. Previous output buffers
// are not freed. Non-NULL outputs must be writable and disjoint from inputs;
// they must be either identical or disjoint from each other.
// parse_time: NULL arguments return 0 without writes; outputs may alias each other.
// parse_week_days: NULL input means empty; NULL output returns 0.
// validate_option_name: NULL name is invalid; NULL callback means not predefined.
// The callback must not unwind or modify/invalidate name; ctx is caller-owned.
// has_script: NULL inputs mean empty. parse_category_source: NULL means NZBFile.
// parse_category_source: FeedInfo::CategorySource (0 Auto, 1 NZBFile, 2 FeedFile).
// option_restricted: OptEntry::Restricted, 1 when the value is protected; NULL is 0.
// signedChar is nonzero when the caller's plain char is signed, including
// compiler overrides such as -funsigned-char; Rust's target default may differ.
// A non-NULL name must be readable and NUL-terminated within one allocation,
// and remain unchanged for the duration of the call.
int nzbget_rs_validate_option_name(const char* name, int (*predefined)(void* ctx, const char* name), void* ctx);
int nzbget_rs_option_restricted(const char* name, int signedChar);

// NntpConnection's protocol (rust/src/nntp.rs), driving the C++ connection
// through NzbgetRsNntpIo. Each callback returns 0, or -1 when it threw: the
// exchange stops and the call returns -1 (rethrow then). readLine sets *line
// to the connection's line buffer or NULL; the exchange may cut it at its last
// CR. The login state (authError, authRejected) is read and written back.
// Only one exchange may use a connection at a time; callbacks must not reenter
// it or unwind. A non-NULL line is writable and NUL-terminated, and remains
// valid until the next readLine (including after return to the caller). Only
// readLine may overwrite it. Callback string arguments are borrowed for that
// callback only. Configuration strings remain immutable during the exchange;
// NULL configuration strings mean empty. req/group may alias the line buffer
// and must remain NUL-terminated across reads. Nothing is retained or freed.
// Non-NULL outputs must be writable, mutually disjoint, and not overlap io or
// strings. NULL required arguments or a missing required callback return -1;
// result outputs are written only on success. debug may be NULL.
// request: *answer is the buffer or NULL. handshake (after the socket
// connected): *result 0 connected, 1 the greeting failed (disconnect), 2 the
// login failed. join_group (not in the group yet): *answer, *joined.
typedef struct NzbgetRsNntpIo
{
	void* ctx;
	int (*writeLine)(void* ctx, const char* line);
	int (*readLine)(void* ctx, char** line);
	int (*reportError)(void* ctx, const char* prefix, const char* arg);
	int (*debug)(void* ctx, const char* msg);
	int (*cancelled)(void* ctx, int* cancelled);
	const char* user;
	const char* password;
	const char* name;
	const char* host;
	const char* connHost;
} NzbgetRsNntpIo;
int nzbget_rs_nntp_request(const NzbgetRsNntpIo* io, const char* req, int* authError, int* authRejected, char** answer);
int nzbget_rs_nntp_handshake(const NzbgetRsNntpIo* io, int* authError, int* authRejected, int* result);
int nzbget_rs_nntp_join_group(const NzbgetRsNntpIo* io, const char* group, int* authError, int* authRejected,
	char** answer, int* joined);

typedef struct NzbgetRsCmdline NzbgetRsCmdline;
NzbgetRsCmdline* nzbget_rs_cmdline_new(
	int argc, const char* const* argv,
	void (*write_stdout_cb)(void* ctx, const char* ptr, size_t len), void* ctx);
void nzbget_rs_cmdline_free(NzbgetRsCmdline* cmd);
int nzbget_rs_cmdline_get_flag(const NzbgetRsCmdline* cmd, int field);
const char* nzbget_rs_cmdline_get_str(const NzbgetRsCmdline* cmd, int field);
int nzbget_rs_cmdline_run_client(
	const NzbgetRsCmdline* cmd, const char* control_ip, int control_port,
	const char* control_username, const char* control_password,
	void (*write_stdout_cb)(void* ctx, const char* ptr, size_t len),
	void (*write_perror_cb)(void* ctx, const char* msg), void* ctx);

// FileSystem's file operations (rust/src/fsops.rs, POSIX). error_message:
// writes at most 1022 bytes plus NUL (the original BString capacity argument)
// to a 1024-byte buffer; NULL is a no-op. Buffers returned by these functions
// are Rust-owned and must be freed with nzbget_rs_free, never free/delete.
// Panics abort; they do not unwind across the ABI.
void nzbget_rs_error_message(int errnum, char* buf);
// NULL paths return 0.
int nzbget_rs_file_exists(const char* path);
int nzbget_rs_directory_exists(const char* path);
// 1 on success, 0 on failure; every non-NULL errmsg is initialized (empty on
// success or invalid NULL arguments). errmsg must not overlap the input.
int nzbget_rs_force_directories(const char* path, NzbgetRsBuf* errmsg);

// ParParser's name checks (rust/src/parparser.rs).
// Names are borrowed and read only through the first NUL or 1023 bytes.
// fold takes a byte (0..255), applies the caller's tolower(char), and must not
// unwind. NULL names/fold return 0. Outputs are optional, writable C ints;
// they may alias each other (blocks is written last), and change only on success.
// No ownership transfer or allocation crosses the ABI; panics abort.
// table: glibc's tolower table (*__ctype_tolower_loc()) read for a char of
// the given signedness instead of calling fold per byte; NULL calls fold.
int nzbget_rs_parse_par_filename(const char* name, int confirmed, int* baseLen, int* blocks);
int nzbget_rs_same_par_collection(const char* name1, const char* name2, int confirmed);
int nzbget_rs_find_main_pars(const char* path, int collect,
	void (*addFile)(void* ctx, const char* name, size_t len), void* ctx);

// RarRenamer::Execute (rust/src/rarrenamer.rs): recursively scans dest_dir,
// parses RAR3/RAR5 volumes, and renames misnamed archive sets. Callbacks return
// 0 on success or nonzero if an exception occurred (stopped returns 0 for false,
// 1 for true, or -1 on exception).
typedef struct NzbgetRsRarRenamerRunEnv
{
	void* ctx;
	int (*stopped)(void* ctx);
	int (*progress)(void* ctx, const char* label, int stageProgress);
	int (*message)(void* ctx, int kind, const char* text);
	int (*registerRenamed)(void* ctx, const char* oldBase, const char* newBase);
} NzbgetRsRarRenamerRunEnv;
int nzbget_rs_rar_renamer_execute(
	const NzbgetRsRarRenamerRunEnv* env, const char* destDir, const char* infoName,
	const char* password, const char* ignoreExt, int tlsEnabled, int* renamedCount);

// DirectRenamer's RenameContentAnalyzer (rust/src/renameanalyzer.rs): the PAR2
// packet header and MD5 of a file's first 16 KiB, fed article by article.
// append returns 1 when it recognized a PAR2 header (setId: 32 hex digits and
// a NUL); finish writes the printed MD5 (32 hex digits and a NUL).
// The caller owns these 33-byte output buffers; no buffer is retained or freed.
// Output buffers must not overlap the input or analyzer. NULL outputs discard
// the set id; NULL hash makes finish a no-op. NULL analyzers are accepted as
// no-ops, as are negative lengths and NULL input with positive length. NULL
// input with zero length is an ordinary empty piece (it may recognize a header).
// Use only new/free for analyzer ownership, with exclusive access on each call.
typedef struct NzbgetRsRenameAnalyzer NzbgetRsRenameAnalyzer;
NzbgetRsRenameAnalyzer* nzbget_rs_rename_analyzer_new(void);
void nzbget_rs_rename_analyzer_free(NzbgetRsRenameAnalyzer* a);
void nzbget_rs_rename_analyzer_reset(NzbgetRsRenameAnalyzer* a);
int nzbget_rs_rename_analyzer_append(NzbgetRsRenameAnalyzer* a, const char* buf, int len, char* setId);
void nzbget_rs_rename_analyzer_finish(NzbgetRsRenameAnalyzer* a, char* hash);

// DirectRenamer's new names (rust/src/directrenamer.rs), comparing in the
// current locale. Strings are borrowed for the call; NULL strings are empty.
// direct_regular_name: the index of the par2 hash whose filename the file
// gets, or -1 to keep its name; at writes index's filename and hash (borrowed
// until the call returns) for every index below count; NULL at returns -1.
// direct_par_name: the par2 file's new name (free with nzbget_rs_free), *vol
// counted up for every name tried (NULL vol: from 1); exists gets the full
// path (NULL: nothing exists). format is NULL for classic formatting, or
// writes a borrowed candidate buffer and length (valid until the next
// callback), returning 1 on success, 0 to stop the search. Callbacks must
// not access *vol. direct_need_rename_pars: 1 when the par2 files
// need new names; a NULL filename needs one; NULL pars/fold return 0. fold is
// ParParser::FilenameFold. Callbacks must not throw; panics abort.
typedef void (*NzbgetRsParHashAt)(void* ctx, size_t index, const char** filename, const char** hash);
ptrdiff_t nzbget_rs_direct_regular_name(const char* oldName, const char* hash16k, size_t count,
	NzbgetRsParHashAt at, void* ctx);
NzbgetRsBuf nzbget_rs_direct_par_name(const char* oldName, const char* destDir, const char* setId,
	char separator, int* vol);
typedef struct NzbgetRsParName
{
	const char* filename;
	const char* setId;
} NzbgetRsParName;
int nzbget_rs_direct_need_rename_pars(const NzbgetRsParName* pars, size_t count);

// ParRenamer::Execute, ParRenamer::ListParredFiles, and DirectParLoader::LoadParFile
// (rust/src/parrenamer.rs).
void nzbget_rs_par_renamer_execute(
	const char* infoName, const char* destDir, const char* ignoreExt,
	int detectMissing, char separator, void* ctx,
	int (*isStoppedCb)(void* ctx),
	void (*updateProgressCb)(void* ctx, const char* label, int stage),
	void (*printMessageCb)(void* ctx, int kind, const char* text),
	void (*registerParredCb)(void* ctx, const char* name),
	void (*registerRenamedCb)(void* ctx, const char* oldRel, const char* newRel),
	int* outRenamedCount, int* outHasMissedFiles, int* outHasDamagedParFiles);
int nzbget_rs_par_renamer_list_parred_files(
	const char* parFilename, int64_t maxIndexParSize, void* ctx,
	void (*addCb)(void* ctx, const char* name));
int nzbget_rs_par_renamer_load_par_entries(
	const char* parFilename, void* ctx,
	int (*onLoadedCb)(void* ctx),
	int (*entryCb)(void* ctx, int damaged, const char* name, const char* hash));

// FileSystem's deletes and RenameBak (rust/src/fsdelete.rs). The callbacks
// get borrowed NUL-terminated paths and return -1 for a C++ exception (the
// call then returns -1). pathType: GetPathTypeNoFollow, 0 error, 1 file,
// 2 directory, 3 link, 4 directory link. removeDir / deleteFile / deleteLink
// (type 3 or 4) / fileExists / dirExists / moveFile: 1 done or yes. list
// calls add for each name of DirBrowser's snapshot; lastError calls set with
// GetLastErrorMessage for the call that failed last. NULL callbacks mean
// errors, empty lists, nothing done. errmsg / newName (NULL: not written)
// get a buffer to free with nzbget_rs_free; an empty errmsg was cleared.
// A NULL ops table returns 0 and writes empty output buffers. Output slots
// must not hold an allocation needing release. Callbacks must not unwind;
// sink and add/set are valid only during their list/lastError call. Strings
// passed to add/set are copied before that function returns.
int nzbget_rs_delete_directory_with_content(const char* dir, char separator, NzbgetRsBuf* errmsg);

// NzbFile (rust/src/nzbfile.rs), under the C library's ctype: classes are
// isspace/isdigit's classes of bytes 0..255 (masks space and digit), lower
// is tolower's table of -128..255 at index 0, charSigned whether char is
// signed (glibc's __ctype_b_loc/__ctype_tolower_loc tables fit). file_totals:
// a file's count segments (parts and sizes in the nzb's order) as AddFileInfo
// makes them: 1 with the kept segments' indexes in part order (of two with
// one number the later) in order[0..totals->kept], the subject's declared
// tail counted when the nzb plainly stops short (cutShort), and the sizes;
// 0 (outputs untouched) when no segment is left. duplicate_filenames:
// HasDuplicateFilenames over count names and subjects (NULL entries empty).
// declared_parts: DeclaredParts, the "(1/N)" count ending a subject or 0.
typedef struct
{
	const unsigned short* classes;
	unsigned short space;
	unsigned short digit;
	const int* lower;
	int charSigned;
} NzbgetRsCtype;

// UrlCoordinator (rust/src/urlcoord.rs). dnzb_header: UrlDownloader's
// X-DNZB headers; 1 with *start and *length the trimmed value of
// X-DNZB-Category within line (nothing is allocated), 2 with *name "*DNZB:"
// and the header's name (cut to 99 bytes) and *value its value after the
// colon's spaces, right-trimmed, Latin-1 to UTF-8; 0 for another line
// (outputs untouched). Free name and value with nzbget_rs_free; NULL outputs
// aren't written, a NULL line is empty. completed: the URL and delete
// statuses UrlCompleted leaves for the downloader's status
// (WebDownloader::EStatus) and whether the fetch is retried; NULL pointers
// read as 0 and aren't written.
int nzbget_rs_url_dnzb_header(const char* line, size_t* start, size_t* length, NzbgetRsBuf* name, NzbgetRsBuf* value);

// ---- dupesearch (daemon/dupesearch/) ----
// DonorScore (rust/src/donorscore.rs): Base, Target (twin 0/1) and Rerank;
// rerank writes n scores parallel to entries into out (both NULL for n 0).
// ReleaseName (rust/src/releasename.rs): Clean, Normalize, ShortQuery, Readable,
// SameRelease; Parse returns the attributes encoded: per set (resolution,
// quality, codec, bitDepth, audio, channels, network, edition, hdr) a u32
// count and its strings, each a u32 length and bytes; then title, group, the
// seasons and episodes (u32 count, i32 each), year (i32), repack and proper
// (one byte each); all little-endian. Names may be NULL (empty).
NzbgetRsBuf nzbget_rs_releasename_normalize(const char* name, size_t len);
int nzbget_rs_releasename_readable(const char* name, size_t len);
int nzbget_rs_releasename_same_release(const char* a, size_t alen, const char* b, size_t blen);

// Posting (rust/src/posting.rs): MakeSketch (out holds up to 64), SameSketch,
// SamePosting (both lists sorted), ListingMismatch; GroupListings and
// OrderPostings write groups of indices into the listings: indices group after
// group, group_sizes each group's length, *groups the number of groups (both
// arrays writable for n); they return the indices written.
typedef struct NzbgetRsListing
{
	const char* link;
	size_t linkLen;
	int64_t size;
	int32_t grabs;
	int64_t date;
} NzbgetRsListing;
void nzbget_rs_posting_sketch(const char* const* ids, const size_t* lens, size_t n, uint32_t* out, size_t* outLen);
int nzbget_rs_posting_same_sketch(const uint32_t* a, size_t na, const uint32_t* b, size_t nb);
int nzbget_rs_posting_same_posting(const char* const* a, const size_t* alens, size_t na,
	const char* const* b, const size_t* blens, size_t nb);
int nzbget_rs_posting_listing_mismatch(int64_t listingSize, int64_t nzbBytes);
size_t nzbget_rs_posting_order(const NzbgetRsListing* listings, size_t n, int64_t pickBytes, int maxPostings,
	size_t* indices, size_t* groupSizes, size_t* groups);

// Native duplicate-discovery identity. Inputs are caller-owned; NULL is empty.
// Inner result is size:u64 LE, extension-length:u32 LE, extension, name; empty
// means unknown. The caller releases it with nzbget_rs_free.
typedef struct
{
    uint64_t bytes;
    const char* name;
    size_t name_len;
    const char* extension;
    size_t extension_len;
} NzbgetRsDupeIdentityInput;
int nzbget_rs_dupe_candidate(const char* title, size_t titleLen, uint64_t bytes,
    const char* name, size_t nameLen, uint64_t size);
int nzbget_rs_dupe_lenient_ok(const char* title, size_t titleLen, const char* mainName, size_t mainLen,
    const NzbgetRsDupeIdentityInput* mine, const NzbgetRsDupeIdentityInput* theirs);

// Dead-posting store (rust/src/deadpostings.rs).
struct NzbgetRsDeadPostingsStore;
typedef struct NzbgetRsDeadPostingsStore NzbgetRsDeadPostingsStore;
NzbgetRsDeadPostingsStore* nzbget_rs_deadpostings_store_new(void);
void nzbget_rs_deadpostings_store_free(NzbgetRsDeadPostingsStore* store);
void nzbget_rs_deadpostings_store_set_state_path(
	const NzbgetRsDeadPostingsStore* store, const uint8_t* path, size_t len);
void nzbget_rs_deadpostings_store_load(const NzbgetRsDeadPostingsStore* store, int64_t now);
int32_t nzbget_rs_deadpostings_store_is_dead(
	const NzbgetRsDeadPostingsStore* store, const uint32_t* sketch_ptr, size_t sketch_len, int64_t now);
int32_t nzbget_rs_deadpostings_store_add(
	const NzbgetRsDeadPostingsStore* store, const uint32_t* sketch_ptr, size_t sketch_len, int64_t now, int32_t save);
// Watcher flags are snapshots of daemon objects, not queue mutation requests.
enum NzbgetRsDupeWatchFlags {
    rsDupeEnabled = 1u << 0, rsDupeCheck = 1u << 1, rsDupePresent = 1u << 2,
    rsDupeNzb = 1u << 3, rsDupeHistory = 1u << 4, rsDupeSuccess = 1u << 5,
    rsDupeDeleting = 1u << 6, rsDupeDeleteNone = 1u << 7, rsDupeDeleteManual = 1u << 8,
    rsDupeDeleteDupe = 1u << 9, rsDupeDeleteCopy = 1u << 10, rsDupeDeleteGood = 1u << 11,
    rsDupePostProcess = 1u << 12, rsDupeScoreMode = 1u << 13, rsDupeDonor = 1u << 14,
    rsDupeAutoRedownload = 1u << 15, rsDupeManagedFleet = 1u << 16
};
typedef struct {
    int32_t id, score;
    uint32_t flags;
    const char* key;
    size_t key_len;
} NzbgetRsDupeWatchPeer;
// Same decision with no peers, an existing stamp and unchanged identity.
// Only scalar arguments are read; no queue record needs to be constructed.
#if defined(__GNUC__) || defined(__clang__)
__attribute__((const, nothrow))
#endif
int nzbget_rs_dupe_watch_stamped(uint32_t flags, int32_t score, int64_t now,
    int32_t previousScore, int64_t previousTime);
// Return 0 skip, 1 retry post-processing, 2 search. Inputs caller-owned/readable.
/* This decision function only reads its valid snapshot inputs and cannot unwind. */
#if defined(__GNUC__) || defined(__clang__)
__attribute__((pure, nothrow))
#endif
int nzbget_rs_dupe_watch(const NzbgetRsDupeWatchPeer* pick, const NzbgetRsDupeWatchPeer* peers, size_t count,
    int64_t now, int32_t previousScore, int64_t previousTime, int hasPrevious, int identityChanged);
// An opaque per-search budget (3 * maxDonors; zero means unlimited).
// All concurrent callers must have joined before free.
void* nzbget_rs_dupe_budget_new(uint32_t maxDonors);
void nzbget_rs_dupe_budget_free(void* budget);
int nzbget_rs_dupe_budget_spent(const void* budget);

uint64_t nzbget_rs_dupe_par_block(const char* data, size_t len);
int nzbget_rs_dupe_outer_ok(const char* title, size_t titleLen, const char* name, size_t len);

// Memory Pressure Monitor C ABI (Requirement R8)
typedef enum {
    NZBGET_RS_PRESSURE_NORMAL = 0,
    NZBGET_RS_PRESSURE_CONSTRAINED = 1,
    NZBGET_RS_PRESSURE_CRITICAL = 2
} NzbgetRsPressureTier;

int32_t nzbget_rs_memory_pressure_sample(void);
uint32_t nzbget_rs_memory_pressure_effective_cache(uint32_t configured_mb);
uint32_t nzbget_rs_memory_pressure_effective_par_buffer(uint32_t configured_mb);
void nzbget_rs_memory_pressure_set_mock_root(const char* mock_root);
void nzbget_rs_memory_pressure_trim_heap(void);

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

// C++ subsystem bridge functions exported to Rust
void nzbget_c_set_process_args(int argc, char* argv[], char* argp[]);
void nzbget_c_util_init(void);
int  nzbget_c_nserv_main(int argc, char* argv[]);
int  nzbget_c_phase1_init(bool reloading);
int  nzbget_c_phase2_subsystems_create_and_recover(bool reloading);
int  nzbget_c_phase3_server_activate(void);
void nzbget_c_phase4_shutdown(bool reloading);
bool nzbget_c_has_active_unpack(void);
bool nzbget_c_is_reloading(void);
void nzbget_c_stop_server(bool reloading);
void nzbget_c_reload(void);
void nzbget_c_exit_proc(void);

// ============================================================================
// DupeArticleFallback, ParDamage, DupeProbe, ServerPool, ArticleFetcher &
// ArticleDownloader Hot-Path C ABI (rust/src/dupefallback.rs)
// ============================================================================


typedef struct {
    int64_t offset;
    int64_t size;
} NzbgetRsByteRange;


uint64_t nzbget_rs_dupe_fallback_steps_hash(const int* sizes, size_t count);

typedef struct {
    const char* filename;
    const char* subject;
    int32_t total_articles;
    int64_t size;
    int32_t is_par_file;
    const int32_t* parts;
    const int32_t* sizes;
    size_t article_count;
} NzbgetRsDonorFileSpec;

ptrdiff_t nzbget_rs_dupe_fallback_match_donor_file(
    const NzbgetRsDonorFileSpec* target,
    const char* target_nzb_filename,
    const char* by_content,
    int twin_allowed,
    int twin_only,
    const NzbgetRsDonorFileSpec* donor_files,
    size_t donor_count);





// ============================================================================
// Post-Repair & DupeCoordinator Hot-Path FFI (`rust/src/postrepair.rs`)
// ============================================================================

typedef struct NzbgetRsDupeHistoryCandidate {
    int32_t kind;
    int32_t dupe_mode;
    int32_t dupe_score;
    int32_t is_dupe_success;
    int32_t mark_status;
    int32_t dup_status;
    int32_t delete_status;
    uint32_t full_content_hash;
    int32_t alive_permille;
    int32_t health;
    int32_t critical_health;
    int32_t same_name_or_key;
} NzbgetRsDupeHistoryCandidate;

typedef struct NzbgetRsDupeQueueCandidate {
    int32_t is_self;
    int32_t is_nzb_kind;
    int32_t dupe_mode;
    int32_t dupe_score;
    int32_t same_name_or_key;
} NzbgetRsDupeQueueCandidate;

typedef struct NzbgetRsParSegment {
    int32_t success;
    int64_t offset;
    int32_t size;
    uint32_t crc;
} NzbgetRsParSegment;

typedef struct NzbgetRsUnpackMessageInfo {
    int32_t strip_extracting_dir;
    size_t stripped_suffix_offset;
    int32_t is_insert_disk;
    size_t insert_disk_name_offset;
    size_t insert_disk_name_len;
    int32_t reset_stage_time;
    int32_t has_extracting_progress;
    int32_t has_extracting_from;
    int32_t decrypt_error;
    int32_t password_error;
    int32_t direct_password_error;
    int32_t crc_or_volume_error;
    int32_t all_ok;
} NzbgetRsUnpackMessageInfo;

int nzbget_rs_dupecoord_select_dupe_backup(
    int has_self_nzb, uint32_t self_full_content_hash, int dupe_health,
    const NzbgetRsDupeHistoryCandidate* history,
    const char* const* queued_filenames, size_t history_count,
    const NzbgetRsDupeQueueCandidate* queue, size_t queue_count);

typedef struct NzbgetRsParRepairer NzbgetRsParRepairer;

enum NzbgetRsParResult {
    NZBGET_RS_PAR_RESULT_SUCCESS = 0,
    NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE = 1,
    NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE = 2,
    NZBGET_RS_PAR_RESULT_INVALID_ARGS = 3,
    NZBGET_RS_PAR_RESULT_INSUFFICIENT_CRITICAL_DATA = 4,
    NZBGET_RS_PAR_RESULT_REPAIR_FAILED = 5,
    NZBGET_RS_PAR_RESULT_FILE_IO_ERROR = 6,
    NZBGET_RS_PAR_RESULT_LOGIC_ERROR = 7,
    NZBGET_RS_PAR_RESULT_MEMORY_ERROR = 8,
};

typedef struct NzbgetRsParCallbacks {
    void* ctx;
    void (*on_filename)(void* ctx, const char* name);
    void (*on_progress)(void* ctx, int progress);
    void (*on_done)(void* ctx, const char* name, int available, int total, int target_exists, int is_complete);
    int (*on_quick_verify)(
        void* ctx,
        const char* name,
        uint32_t* out_crc,
        const NzbgetRsParSegment** out_segs,
        size_t* out_seg_len);
    void (*on_quick_verified_inc)(void* ctx);
} NzbgetRsParCallbacks;

NzbgetRsParRepairer* nzbget_rs_par_repairer_new(
    const char* base_dir,
    int threads_to_use,
    int mem_to_use_mb,
    int par_quick,
    int par_scan_full,
    const char* par_ignore_ext);
void nzbget_rs_par_repairer_free(NzbgetRsParRepairer* handle);
void nzbget_rs_par_repairer_cancel(const NzbgetRsParRepairer* handle);
int nzbget_rs_par_repairer_preprocess(
    NzbgetRsParRepairer* handle,
    const char* par_filename,
    const NzbgetRsParCallbacks* cbs);
int nzbget_rs_par_repairer_process(
    NzbgetRsParRepairer* handle,
    int do_repair,
    const NzbgetRsParCallbacks* cbs);
int nzbget_rs_par_repairer_load_extra_par2_file(
    NzbgetRsParRepairer* handle,
    const char* path,
    const NzbgetRsParCallbacks* cbs);
int nzbget_rs_par_repairer_verify_extra_file(
    NzbgetRsParRepairer* handle,
    const char* path,
    int is_split_fragment,
    const NzbgetRsParCallbacks* cbs);
uint32_t nzbget_rs_par_repairer_missing_file_count(const NzbgetRsParRepairer* handle);
uint32_t nzbget_rs_par_repairer_damaged_file_count(const NzbgetRsParRepairer* handle);
uint32_t nzbget_rs_par_repairer_missing_block_count(const NzbgetRsParRepairer* handle);
uint32_t nzbget_rs_par_repairer_recovery_block_count(const NzbgetRsParRepairer* handle);
int64_t nzbget_rs_par_repairer_block_size(const NzbgetRsParRepairer* handle);
NzbgetRsBuf nzbget_rs_par_repairer_creator(const NzbgetRsParRepairer* handle);
size_t nzbget_rs_par_repairer_source_file_count(const NzbgetRsParRepairer* handle);

int nzbget_rs_streaming_par_has_session(int nzb_id);
int nzbget_rs_streaming_par_start(int nzb_id, const char* par2_path, const char* base_dir);
int nzbget_rs_streaming_par_register_queued_volume(
    int nzb_id, uint64_t volume_id, int blocks, int paused);
int nzbget_rs_streaming_par_on_article_finished_by_name(
    int nzb_id,
    const char* name,
    uint64_t offset,
    uint64_t size,
    uint32_t crc32,
    const uint8_t* bytes,
    size_t bytes_len);
int nzbget_rs_streaming_par_on_article_failed_permanently_by_name(
    int nzb_id,
    const char* name,
    uint64_t offset,
    uint64_t size);
int nzbget_rs_streaming_par_on_recovery_volume_arrived(
    int nzb_id,
    const char* path,
    uint64_t queued_id,
    int has_queued_id);
size_t nzbget_rs_streaming_par_take_unpause_ids(
    int nzb_id,
    uint64_t* out_ids,
    size_t max_ids);
void nzbget_rs_streaming_par_forget(int nzb_id);

void nzbget_rs_unpack_parse_message(
    const char* text, const char* unpack_extended_dir, int is_seven_zip,
    NzbgetRsUnpackMessageInfo* out_info);

// ============================================================================
// QueueCoordinator, ArticleWriter, HistoryCoordinator, QueueEditor &
// DownloadInfo Hot-Path FFI (`rust/src/queuehotpath.rs`)
// ============================================================================


// ============================================================================
// DiskState, PrePostProcessor, Scanner & TwinCheck Hot-Path FFI (`rust/src/diskstatepp.rs`)
// ============================================================================


// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================



typedef struct NzbgetRsNntpCache NzbgetRsNntpCache;
NzbgetRsNntpCache* nzbget_rs_nntp_cache_new(void);
void nzbget_rs_nntp_cache_free(NzbgetRsNntpCache* cache);
int nzbget_rs_nntp_processor_handle_line(
    int server_id, const char* data_dir, const char* cache_dir,
    int latency, int speed, const NzbgetRsNntpCache* cache,
    const char* raw_line, void* ctx,
    void (*write_line_cb)(void* ctx, const char* line),
    void (*send_cb)(void* ctx, const char* buf, int size),
    void (*log_cb)(void* ctx, int kind, const char* msg));
typedef struct NzbgetRsNservOpts NzbgetRsNservOpts;
NzbgetRsNservOpts* nzbget_rs_nserv_opts_new(
    int argc, const char* const* argv, void* ctx,
    void (*write_stdout_cb)(const char* data, size_t len),
    void (*log_cb)(int level, const char* msg),
    void (*push_opt_cb)(void* ctx, const char* opt));
int nzbget_rs_nserv_opts_post_setup(
    const NzbgetRsNservOpts* opts, void (*log_cb)(int level, const char* msg));
const char* nzbget_rs_nserv_opts_str(const NzbgetRsNservOpts* opts, int field);
int nzbget_rs_nserv_opts_int(const NzbgetRsNservOpts* opts, int field);
void nzbget_rs_nserv_opts_free(NzbgetRsNservOpts* opts);

typedef struct NzbgetRsProcessArticleItem {
    int64_t segment_offset;
    int32_t segment_size;
    uint32_t crc;
    int32_t part_number;
    int32_t dupe_fallback_round;
    const uint8_t* segment_content;
    const char* result_filename;
} NzbgetRsProcessArticleItem;

int nzbget_rs_awc_article_writer_build_output_filenames(
    const char* tmp_dir, const char* dest_dir, char sep, int file_id, int part_num,
    int direct_write, int force_direct_write, const char* existing_output_filename,
    int output_initialized, const char* file_filename,
    NzbgetRsBuf* out_result_filename, NzbgetRsBuf* out_temp_filename,
    NzbgetRsBuf* out_output_filename, int* out_has_output_filename,
    int* out_update_file_info_output_filename);
int nzbget_rs_awc_article_writer_execute_setup_output_file(
    const char* dest_dir, size_t dest_dir_len,
    const char* filename, size_t filename_len,
    char sep, int raw_article, int direct_write, int force_direct_write,
    const char* output_filename, size_t output_filename_len,
    void* cb_ctx,
    int (*open_file_cb)(void* ctx, const char* path, int read_write),
    NzbgetRsBuf* out_final_path, NzbgetRsBuf* out_temp_path,
    NzbgetRsBuf* out_updated_output_filename);
void nzbget_rs_awc_article_writer_execute_cleanup_old_data(
    int direct_write,
    const char* nzb_dest_dir, size_t nzb_dest_dir_len,
    const char* final_output_path, size_t final_output_path_len,
    const char* output_filename, size_t output_filename_len,
    int is_hard_linked, int inter_dir_empty, char sep1, char sep2,
    const int* dupe_fallback_rounds, const char* const* result_filenames, size_t articles_len,
    void* log_ctx, void (*log_cb)(void* ctx, int is_debug, const char* msg),
    int* out_clear_output_filename);
uint32_t nzbget_rs_awc_article_writer_process_articles(
    const NzbgetRsProcessArticleItem* items, size_t items_len,
    size_t total_articles_count,
    const char* info_filename, size_t info_filename_len,
    const char* final_output_path, size_t final_output_path_len,
    int raw_article, int direct_write, int skip_disk_write, int is_yenc,
    void* ctx,
    int64_t (*io_cb)(void* ctx, int op, int64_t val, const void* ptr, size_t len),
    int* out_commit_error, int* out_failed_delta);
int nzbget_rs_awc_article_writer_move_completed_files(
    const char* dest_dir, const char* old_dest_dir,
    const char* const* completed_filenames, size_t completed_len,
    void* ctx,
    int (*cb)(void* ctx, const char* err_msg_or_null));
int nzbget_rs_awc_article_writer_create_output_file(
    const char* output_filename, int64_t size, NzbgetRsBuf* out_err);

typedef struct NzbgetRsFlushArticleItem {
    int64_t segment_offset;
    int32_t segment_size;
    const uint8_t* segment_content;
    const char* result_filename;
} NzbgetRsFlushArticleItem;

void nzbget_rs_awc_article_writer_flush_cache(
    const NzbgetRsFlushArticleItem* items, size_t items_len,
    int direct_write, int skip_disk_write,
    const char* output_filename, size_t output_filename_len,
    void* ctx,
    int (*cb)(void* ctx, int op, size_t idx, const char* msg),
    int* out_flushed_articles, int64_t* out_flushed_size);
int nzbget_rs_awc_article_writer_post_process_rename_and_hardlink(
    const char* original_filename, size_t original_filename_len,
    const char* current_filename,
    const char* nzb_dest_dir, size_t nzb_dest_dir_len,
    const char* file_output_filename, size_t file_output_filename_len,
    const char* writer_output_filename, size_t writer_output_filename_len,
    int is_hard_linked, int inter_dir_empty,
    void* ctx, void (*log_err_cb)(void* ctx, const char* msg),
    NzbgetRsBuf* out_updated_output_filename, int* out_has_updated_output_filename);
void nzbget_rs_awc_article_writer_finish_disk_ops(
    int success, int skip_disk_write, int raw_article, int direct_write, int has_cached_data,
    const char* temp_filename, size_t temp_filename_len,
    const char* result_filename, size_t result_filename_len,
    void* ctx, void (*log_err_cb)(void* ctx, const char* msg));



/* Consolidated WebDownloader & StatMeter FFI (`webdownload.rs`, `statmeter.rs`) */

// DownloadInfo & DiskState C ABI (rust/src/downloadinfo.rs, rust/src/diskstatepp.rs)



typedef struct {
    int64_t remaining_size;
    int64_t forced_size;
    int64_t article_cache;
    const char* memory_pressure;
    int effective_article_cache_mb;
    int effective_par_buffer_mb;
    int64_t download_rate;
    int download_limit;
    int thread_count;
    int post_job_count;
    int url_count;
    int up_time_sec;
    int download_time_sec;
    int64_t all_bytes;
    int64_t month_bytes;
    int64_t day_bytes;
    int download_paused;
    int server_stand_by;
    int post_paused;
    int scan_paused;
    int quota_reached;
    int64_t free_disk_space;
    int64_t total_disk_space;
    int inter_dir_empty;
    int64_t free_inter_disk_space;
    int64_t total_inter_disk_space;
    int server_time;
    int resume_time;
    int feed_active;
    int queued_scripts;
} NzbgetRsRpcStatusInput;

typedef struct {
    int id;
    int active;
} NzbgetRsRpcServerStatus;

typedef struct {
    int id;
    int kind;
    int time;
    const char* text;
} NzbgetRsRpcLogEntry;

typedef struct {
    int id;
    int64_t file_size;
    int64_t remaining_size;
    int post_time;
    int filename_confirmed;
    int paused;
    int nzb_id;
    const char* nzb_name;
    const char* nzb_filename;
    const char* subject;
    const char* filename;
    const char* dest_dir;
    const char* category;
    int priority;
    int active_downloads;
    int progress;
} NzbgetRsRpcFileItem;

typedef struct {
    int id;
    int kind;
    const char* name;
    const char* url;
    const char* filename;
    const char* dest_dir;
    const char* final_dir;
    const char* category;
    int par_status;
    int unpack_status;
    int move_status;
    int script_status;
    int delete_status;
    int mark_status;
    int url_status;
    int64_t size;
    int file_count;
    int min_time;
    int max_time;
    int total_articles;
    int current_success_articles;
    int current_failed_articles;
    int dupe_recovered_articles;
    int64_t dupe_recovered_bytes;
    int dupe_recovered_holes;
    int live_repairing;
    int health;
    int critical_health;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    int64_t downloaded_size;
    int download_sec;
    int post_total_sec;
    int has_post_start_time;
    int64_t cur_time;
    int64_t post_start_time;
    int par_sec;
    int repair_sec;
    int unpack_sec;
    int message_count;
    int cached_message_count;
    int extra_par_blocks;
} NzbgetRsRpcNzbInfoInput;

typedef struct {
    const char* name;
    const char* value;
} NzbgetRsRpcParamItem;

typedef struct {
    const char* name;
    int status;
} NzbgetRsRpcScriptItem;

typedef struct {
    int server_id;
    int success_articles;
    int failed_articles;
} NzbgetRsRpcServerStatItem;

typedef struct {
    const char* name;
    const char* value;
    int restricted;
} NzbgetRsRpcConfigItem;

typedef struct {
    const char* title;
    const char* filename;
    const char* url;
    int64_t size;
    int size_mb;
    const char* category;
    const char* add_category;
    int pause_nzb;
    int priority;
    int time;
    int match_status;
    int match_rule;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    int status;
} NzbgetRsRpcFeedItem;

typedef struct {
    uint32_t failed;
    uint32_t success;
} NzbgetRsRpcArticlePair;

typedef struct {
    int server_id;
    int data_time;
    int first_day;
    int64_t total_bytes;
    int64_t custom_bytes;
    int custom_time;
    int counters_reset_time;
    int sec_slot;
    int min_slot;
    int hour_slot;
    int day_slot;
    int include_seconds;
    const int64_t* bytes_per_seconds;
    size_t bytes_per_seconds_len;
    int include_minutes;
    const int64_t* bytes_per_minutes;
    size_t bytes_per_minutes_len;
    int include_hours;
    const int64_t* bytes_per_hours;
    size_t bytes_per_hours_len;
    int include_days;
    const int64_t* bytes_per_days;
    size_t bytes_per_days_len;
    int include_articles;
    const NzbgetRsRpcArticlePair* articles_per_days;
    size_t articles_per_days_len;
} NzbgetRsRpcServerVolumeItem;

typedef struct {
    int nzb_id;
    const char* name;
    int rank;
    int alive;
    const char* status;
    int same_as;
    int same_as_rank;
    const char* reason;
} NzbgetRsRpcFleetMember;

typedef struct {
    const char* name;
    int is_url;
    const char* data;
    size_t data_len;
} NzbgetRsRpcFleetReqMember;

typedef struct {
    int id;
    int status;
    uint32_t crc;
    int par_file;
    const char* hash16k;
    const char* par_set_id;
    const char* filename;
    const char* origname;
} NzbgetRsDiskCompletedFile;

typedef struct {
    const char* name;
    const char* value;
} NzbgetRsDiskParam;

typedef struct {
    int status;
    const char* name;
} NzbgetRsDiskScriptStatus;

typedef struct {
    int server_id;
    int success_articles;
    int failed_articles;
} NzbgetRsDiskServerStat;

typedef struct {
    int id;
    int paused;
    int extra_priority;
} NzbgetRsDiskFileStub;

typedef struct {
    int id;
    int kind;
    const char* url;
    const char* filename;
    const char* dest_dir;
    const char* final_dir;
    const char* hard_link_path;
    const char* meta_name;
    const char* queued_filename;
    const char* name;
    const char* category;
    int priority;
    int has_post_info;
    int post_stage;
    int delete_paused;
    int many_dupe_files;
    int feed_id;
    int par_status;
    int unpack_status;
    int move_status;
    int par_rename_status;
    int rar_rename_status;
    int direct_rename_status;
    int delete_status;
    int mark_status;
    int url_status;
    int unpack_cleaned_up_disk;
    int health_paused;
    int add_url_paused;
    int file_count;
    int parked_file_count;
    int message_count;
    int min_time;
    int max_time;
    int par_full;
    int force_par_full;
    int force_repair;
    int extra_par_blocks;
    uint32_t full_content_hash;
    uint32_t filtered_content_hash;
    int64_t size;
    int64_t success_size;
    int64_t failed_size;
    int64_t par_size;
    int64_t par_success_size;
    int64_t par_failed_size;
    int total_articles;
    int success_articles;
    int failed_articles;
    const char* dupe_key;
    int dupe_mode;
    int dupe_score;
    int dupe_hint;
    int64_t downloaded_size;
    int download_sec;
    int post_total_sec;
    int par_sec;
    int repair_sec;
    int unpack_sec;
    int skip_script_processing;
    int skip_disk_write;
    int desired_server_id;
} NzbgetRsDiskNzbInfoInput;

typedef struct {
    int part_number;
    int size;
    const char* message_id;
} NzbgetRsDiskArticleInfo;

typedef struct {
    const char* subject;
    const char* filename;
    const char* origname;
    const char* output_filename;
    int filename_confirmed;
    int time;
    int output_initialized;
    int64_t size;
    int64_t missed_size;
    int par_file;
    int total_articles;
    int missed_articles;
} NzbgetRsDiskFileInfoInput;

typedef struct {
    int saved_status;
    int64_t segment_offset;
    int segment_size;
    uint32_t crc;
    int staged_fallback;
} NzbgetRsDiskArticleState;

typedef struct {
    int success_articles;
    int failed_articles;
    int64_t remaining_size;
    int64_t success_size;
    int64_t failed_size;
    const char* filename;
    const char* hash16k;
    const char* par_set_id;
    int par_file;
    int64_t decoded_file_size;
    int output_initialized;
} NzbgetRsDiskFileStateInput;

typedef struct {
    int64_t offset;
    int64_t size;
} NzbgetRsDiskStreamHole;

typedef struct {
    int file_id;
    int64_t decoded_file_size;
    int64_t failed_size;
    int64_t missed_size;
    int par_file;
    int failed_articles;
    uint64_t steps_hash;
    const char* filename;
    const NzbgetRsDiskStreamHole* holes;
    size_t holes_len;
} NzbgetRsDiskRepairJob;

typedef struct {
    int first_day;
    int data_time;
    int custom_time;
    int counters_reset_time;
    int64_t total_bytes;
    int64_t custom_bytes;
    const int64_t* bytes_per_seconds;
    size_t bytes_per_seconds_len;
    const int64_t* bytes_per_minutes;
    size_t bytes_per_minutes_len;
    const int64_t* bytes_per_hours;
    size_t bytes_per_hours_len;
    const int64_t* bytes_per_days;
    size_t bytes_per_days_len;
    const NzbgetRsRpcArticlePair* articles_per_days;
    size_t articles_per_days_len;
} NzbgetRsDiskVolumeStatInput;

NzbgetRsBuf nzbget_rs_cmdline_format_usage(
    const char* base_name, char path_sep, int include_daemon);

size_t nzbget_rs_dupe_fallback_borrowed_par2_mismatches(
    const char* path, const char* dest_dir, const char* filename,
    const int32_t* statuses, const int32_t* donor_ids, const int32_t* fallback_rounds,
    const int64_t* segment_offsets, const int32_t* segment_sizes, size_t article_count,
    int32_t* out_indices, size_t out_cap);

void nzbget_rs_df_build_newznab_queries(
    const char* title_ptr, size_t title_len,
    const char* imdb_ptr, size_t imdb_len,
    const char* tvdb_ptr, size_t tvdb_len,
    void* ctx,
    void (*visit)(
        void* ctx,
        const char* const* keys, const size_t* key_lens,
        const char* const* vals, const size_t* val_lens,
        size_t param_count));
void nzbget_rs_twin_visit_par2_sigs(
    const uint8_t* data, size_t len, void* ctx,
    void (*visit)(
        void* ctx, uint64_t length,
        const char* md5_ptr, size_t md5_len,
        const char* name_ptr, size_t name_len));
void nzbget_rs_twin_visit_nzb_entries(
    const char* path, void* ctx,
    void (*on_entry)(
        void* ctx, int64_t size, int32_t par2,
        const char* fname_ptr, size_t fname_len,
        const char* subj_ptr, size_t subj_len),
    void (*on_group)(void* ctx, const char* group_ptr, size_t group_len),
    void (*on_segment)(void* ctx, int64_t bytes, const char* id_ptr, size_t id_len));

NzbgetRsBuf nzbget_rs_stream_build_donor_key(
    const char* password,
    const char* const* file_names_ptr,
    const int64_t* file_sizes_ptr,
    const uint8_t* file_is_par_ptr,
    const size_t* file_group_counts_ptr,
    size_t file_count,
    const char* const* groups_ptr,
    size_t total_groups,
    const size_t* file_article_counts_ptr,
    const int32_t* art_part_numbers_ptr,
    const int32_t* art_sizes_ptr,
    const char* const* art_message_ids_ptr,
    size_t total_articles);
int32_t nzbget_rs_twin_compare_par2_data(
    const char* const* own_names_ptr,
    const char* const* own_md5s_ptr,
    size_t own_count,
    const char* own_print_ptr,
    const uint8_t* twin_data_ptr,
    size_t twin_data_len);
int nzbget_rs_newznab_parse_response(
    const uint8_t* xml_ptr, size_t xml_len, void* ctx,
    void (*on_error)(void* ctx, int error_code, const uint8_t* error_text_ptr, size_t error_text_len),
    void (*on_item)(void* ctx, const uint8_t* title_ptr, size_t title_len,
        const uint8_t* link_ptr, size_t link_len, int64_t size, int32_t grabs,
        int64_t date, const uint8_t* indexer_ptr, size_t indexer_len));
typedef struct RsNzbSummaryScalars {
    int32_t files;
    int64_t total_bytes;
    int32_t data_files;
    int64_t data_bytes;
    uint64_t listed_parts;
    uint64_t declared_parts;
    uint64_t data_listed_parts;
    uint64_t data_declared_parts;
    uint64_t article_bytes;
    int32_t geometry_valid;
} RsNzbSummaryScalars;
int nzbget_rs_nzb_reader_parse(
    const uint8_t* xml_ptr, size_t xml_len, RsNzbSummaryScalars* out_scalars,
    void* ctx,
    void (*on_strings)(void* ctx,
        const uint8_t* poster_ptr, size_t poster_len,
        const uint8_t* main_name_ptr, size_t main_name_len,
        const uint8_t* par_name_ptr, size_t par_name_len,
        const uint8_t* par_id_ptr, size_t par_id_len,
        int64_t par_bytes),
    void (*on_slice)(void* ctx, int kind,
        const uint8_t* ptr1, size_t len1,
        const uint8_t* ptr2, size_t len2,
        int64_t bytes),
    void (*on_recovery_bytes)(void* ctx, const uint64_t* ptr, size_t len));
int nzbget_rs_posting_sketch_of_file(
    const char* path, uint32_t* out, size_t cap, size_t* out_len);
int nzbget_rs_dupe_util_write_atomic(
    const char* path, size_t path_len, const char* data, size_t data_len);
NzbgetRsBuf nzbget_rs_dupe_util_read_all(const char* path, size_t path_len);
void nzbget_rs_dqh_dupesearch_visit_expand_queries(
    const char* name, size_t name_len, void* ctx,
    void (*cb)(void* ctx, const char* q_ptr, size_t q_len));

typedef struct NzbgetRsParDamageQueuedFile {
    const char* filename;
    int confirmed;
    int64_t size;
} NzbgetRsParDamageQueuedFile;

typedef struct NzbgetRsParDamageCompletedFile {
    const char* filename;
    int not_failed;
} NzbgetRsParDamageCompletedFile;

typedef struct NzbgetRsParDamageVerdict {
    int known;
    uint64_t block_size;
    int damaged_blocks;
    int recovery_blocks;
    int64_t done_bytes;
    int64_t total_bytes;
    int projected_blocks;
} NzbgetRsParDamageVerdict;

void nzbget_rs_par_damage_file_completed(
    int nzb_id,
    int is_par,
    int failed_plus_missed,
    int64_t decoded_size,
    int success_articles,
    int64_t file_size,
    const char* filename,
    const NzbgetRsByteRange* holes,
    size_t holes_count);

int nzbget_rs_par_damage_has_damage(int nzb_id);

void nzbget_rs_par_damage_judge(
    int nzb_id,
    const char* dest_dir,
    int64_t nzb_size,
    int64_t nzb_par_size,
    const NzbgetRsParDamageQueuedFile* queued,
    size_t queued_count,
    const NzbgetRsParDamageCompletedFile* completed,
    size_t completed_count,
    NzbgetRsParDamageVerdict* out_verdict);

int nzbget_rs_par_damage_first_report(int nzb_id);
void nzbget_rs_par_damage_forget(int nzb_id);

int nzbget_rs_move_controller_move_files(
    const char* inter_dir,
    const char* dest_dir,
    void* ctx,
    int (*is_stopped_cb)(void* ctx),
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*rename_cb)(void* ctx, const char* old_rel, const char* new_rel));

void nzbget_rs_move_controller_remove_stale_hardlinks(
    const char* hard_link_path,
    const char* dest_dir,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg));

int nzbget_rs_cleanup_controller_run(
    const char* dest_dir,
    const char* final_dir,
    const char* ext_cleanup_disk,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg),
    int* out_cleanup_status);

int nzbget_rs_post_renamer_run(
    int is_post_unpack,
    const char* meta_name,
    const char* nzb_name,
    const char* dst_dir,
    const char* ignore_ext,
    void* ctx,
    void (*info_name_cb)(void* ctx, const char* info_name),
    int (*is_stopped_cb)(void* ctx),
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*rename_cb)(void* ctx, const char* old_name, const char* new_name));

typedef struct NzbgetRsHealthServerInput {
    int id;
    int active;
    const char* name;
    size_t name_len;
    int level;
    int optional;
    int group;
    const char* host;
    size_t host_len;
    int port;
    const char* username;
    size_t username_len;
    const char* password;
    size_t password_len;
    int tls;
    const char* cipher;
    size_t cipher_len;
    int max_connections;
    int retention;
    uint32_t cert_level;
    int ip_version;
} NzbgetRsHealthServerInput;

typedef struct NzbgetRsHealthTaskInput {
    int id;
    int hours;
    int minutes;
    int week_days_bits;
} NzbgetRsHealthTaskInput;

typedef struct NzbgetRsHealthCategoryInput {
    const char* name;
    size_t name_len;
    int unpack;
} NzbgetRsHealthCategoryInput;

typedef struct NzbgetRsHealthFeedInput {
    int id;
    const char* name;
    size_t name_len;
    const char* url;
    size_t url_len;
    int interval;
    uint32_t cert_level;
} NzbgetRsHealthFeedInput;

typedef struct NzbgetRsSystemHealthConfig {
    const char* main_dir;
    size_t main_dir_len;
    const char* dest_dir;
    size_t dest_dir_len;
    const char* inter_dir;
    size_t inter_dir_len;
    const char* nzb_dir;
    size_t nzb_dir_len;
    const char* queue_dir;
    size_t queue_dir_len;
    const char* web_dir;
    size_t web_dir_len;
    const char* temp_dir;
    size_t temp_dir_len;
    const char* const* script_dir_ptrs;
    const size_t* script_dir_lens;
    size_t script_dir_count;
    const char* config_template;
    size_t config_template_len;
    const char* log_file;
    size_t log_file_len;
    const char* options_log_file;
    size_t options_log_file_len;
    const char* cert_store;
    size_t cert_store_len;
    const char* cert_store_path;
    size_t cert_store_path_len;
    const char* config_file;
    size_t config_file_len;
    const char* lock_file;
    size_t lock_file_len;
    int write_log;
    int cert_check;
    int daemon_mode;
    const NzbgetRsHealthServerInput* servers;
    size_t server_count;
    int output_mode;
    int update_interval;
    int curses_nzb_name;
    int curses_group;
    int curses_time;
    int rotate_log;
    int log_buffer;
    int crash_dump;
    int time_correction;
    int article_retries;
    int hc_dupe;
    int dupe_search;
    int article_interval;
    int article_timeout;
    int article_read_chunk_size;
    int url_retries;
    int url_interval;
    int url_timeout;
    int url_connections;
    int remote_timeout;
    int download_rate;
    int monthly_quota;
    int quota_start_day;
    int daily_quota;
    int flush_queue;
    int continue_partial;
    int propagation_delay;
    int article_cache;
    int direct_write;
    int write_buffer;
    int file_naming;
    int post_strategy;
    int disk_space;
    int nzb_cleanup_disk;
    int keep_history;
    int feed_history;
    int skip_write;
    int raw_article;
    const char* control_ip;
    size_t control_ip_len;
    int control_port;
    const char* control_username;
    size_t control_username_len;
    const char* control_password;
    size_t control_password_len;
    const char* restricted_username;
    size_t restricted_username_len;
    const char* restricted_password;
    size_t restricted_password_len;
    const char* add_username;
    size_t add_username_len;
    const char* add_password;
    size_t add_password_len;
    int form_auth;
    int secure_control;
    const char* secure_cert;
    size_t secure_cert_len;
    const char* secure_key;
    size_t secure_key_len;
    int secure_port;
    int cert_store_path_empty;
    const char* daemon_username;
    size_t daemon_username_len;
    int umask;
    int append_category_dir;
    int nzb_dir_interval;
    int nzb_dir_file_age;
    int dupe_check;
    int health_check;
    int crc_check;
    int par_check;
    int par_repair;
    int par_quick;
    int par_buffer;
    int par_threads;
    size_t hw_threads;
    int par_rename;
    int rar_rename;
    int direct_rename;
    int hard_linking;
    int par_time_limit;
    int unpack;
    int direct_unpack;
    int unpack_pause_queue;
    const char* unrar_path;
    size_t unrar_path_len;
    const char* seven_zip_path;
    size_t seven_zip_path_len;
    const char* ext_cleanup_disk;
    size_t ext_cleanup_disk_len;
    const char* unpack_pass_file;
    size_t unpack_pass_file_len;
    const char* unpack_ignore_ext;
    size_t unpack_ignore_ext_len;
    const NzbgetRsHealthTaskInput* tasks;
    size_t task_count;
    const NzbgetRsHealthCategoryInput* categories;
    size_t category_count;
    const NzbgetRsHealthFeedInput* feeds;
    size_t feed_count;
} NzbgetRsSystemHealthConfig;

NzbgetRsBuf nzbget_rs_system_health_diagnose(
    const NzbgetRsSystemHealthConfig* cfg,
    int format_mode,
    void* ctx,
    void (*log_cb)(void* ctx, int severity, const char* msg));

/* Extension & Script Subsystem End-to-End C ABI (`rust/src/extension.rs`, `rust/src/options.rs`) */
void nzbget_rs_options_convert_old_options(
    const char* const* names, const char* const* values, size_t count,
    void* ctx,
    void (*update_cb)(void* ctx, size_t idx, const char* name, const char* value));


NzbgetRsBuf nzbget_rs_ext_mgr_prepare_update(
    const char* ext_name, int found, int64_t use_count,
    const char* target_path, const char* archive_path,
    NzbgetRsBuf* out_backup_path);

NzbgetRsBuf nzbget_rs_ext_mgr_finish_update(
    const char* target_path, const char* backup_path,
    const char* archive_path, const char* install_err);

NzbgetRsBuf nzbget_rs_ext_mgr_delete_extension(
    const char* ext_name, int found, int64_t use_count,
    const char* target_path);

NzbgetRsBuf nzbget_rs_ext_mgr_install_extension(
    const char* file_path, const char* dest_path,
    void (*log_cb)(void* ctx, int kind, const char* msg), void* log_ctx);

void nzbget_rs_nzb_script_prepare_env(
    const char* script_name, int has_parameters,
    const char* const* param_names, const char* const* param_values, size_t param_count,
    void* ctx,
    void (*set_env_special_cb)(void* ctx, const char* prefix, const char* name, const char* value),
    void (*prepare_options_cb)(void* ctx, const char* strip_prefix));

void nzbget_rs_scan_script_prepare_env(
    const char* nzb_filename, const char* base_filename,
    const char* url, const char* nzb_name, const char* category,
    int priority, int add_top, int add_paused,
    const char* dupe_key, int dupe_score, int dupe_mode,
    const char* directory, char path_sep,
    void* ctx,
    void (*set_env_cb)(void* ctx, const char* name, const char* value),
    void (*set_int_env_cb)(void* ctx, const char* name, int value));

typedef struct NzbgetRsPostScriptEnv {
    int nzb_id;
    const char* nzb_name;
    const char* dest_dir;
    const char* filename;
    const char* queued_filename;
    const char* url;
    const char* final_dir;
    const char* category;
    int health;
    int critical_health;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    const char* status;
    int script_status;
    int par_status;
    int unpack_status;
    int delete_status;
    int mark_status;
    int total_articles;
    int success_articles;
    int failed_articles;
    const int* server_ids;
    const int* server_success_articles;
    const int* server_failed_articles;
    size_t server_count;
} NzbgetRsPostScriptEnv;

void nzbget_rs_post_script_prepare_env(
    const NzbgetRsPostScriptEnv* env, void* ctx,
    void (*set_env_cb)(void* ctx, const char* name, const char* value),
    void (*set_int_env_cb)(void* ctx, const char* name, int value));

typedef struct NzbgetRsQueueScriptEnv {
    const char* nzb_name;
    int nzb_id;
    const char* filename;
    const char* dest_dir;
    const char* queued_filename;
    const char* url;
    const char* category;
    int priority;
    const char* dupe_key;
    int dupe_score;
    int dupe_mode;
    int event;
    int delete_status;
    int url_status;
    int mark_status;
} NzbgetRsQueueScriptEnv;

void nzbget_rs_queue_script_prepare_env(
    const NzbgetRsQueueScriptEnv* env, void* ctx,
    void (*set_env_cb)(void* ctx, const char* name, const char* value),
    void (*set_int_env_cb)(void* ctx, const char* name, int value));

/* ============================================================================
 * Feed Subsystem (`rust/src/feed.rs`)
 * ============================================================================ */

typedef struct NzbgetRsFeedFileParser NzbgetRsFeedFileParser;

typedef struct NzbgetRsParsedFeedItemView {
    const char* title;
    const char* filename;
    const char* url;
    const char* category;
    const char* description;
    const char* season;
    const char* episode;
    int has_season;
    int has_episode;
    int64_t time;
    int64_t size;
    int imdb_id;
    int rage_id;
    int tvdb_id;
    int tvmaze_id;
    const char* const* attr_names;
    const char* const* attr_values;
    size_t attr_count;
} NzbgetRsParsedFeedItemView;

NzbgetRsFeedFileParser* nzbget_rs_feed_file_parser_parse_file(const char* filename);
void nzbget_rs_feed_file_parser_free(NzbgetRsFeedFileParser* parser);
size_t nzbget_rs_feed_file_parser_finalize(NzbgetRsFeedFileParser* parser);
int nzbget_rs_feed_file_parser_get_item(
    const NzbgetRsFeedFileParser* parser, size_t index,
    NzbgetRsParsedFeedItemView* out_view);

// NzbFileParser, UrlCoordinator & DirectRenamer End-to-End FFI
typedef struct NzbgetRsNzbFileParser NzbgetRsNzbFileParser;
typedef int (*NzbgetRsAllocFileIdCb)(void* ctx);

typedef struct
{
    int file_count;
    int total_articles;
    int failed_articles;
    int cur_failed_articles;
    int64_t size;
    int64_t remaining_size;
    int64_t failed_size;
    int64_t cur_failed_size;
    int64_t par_size;
    int64_t par_failed_size;
    int64_t par_cur_failed_size;
    int remaining_par_count;
    uint32_t full_content_hash;
    uint32_t filtered_content_hash;
    int many_dupe_files;
    const char* password;
    const char* meta_name;
    const char* meta_title;
    const char* category;
    size_t kept_files_count;
    size_t messages_count;
} NzbgetRsNzbFileSummary;

typedef struct
{
    int id;
    const char* subject;
    const char* filename;
    int time;
    int total_articles;
    int missed_articles;
    int64_t size;
    int64_t remaining_size;
    int64_t missed_size;
    int par_file;
    int cut_short;
    const char* const* groups_ptrs;
    size_t groups_count;
    const int* article_parts;
    const int64_t* article_sizes;
    const char* const* article_msg_ids;
    size_t articles_count;
} NzbgetRsNzbKeptFileView;

NzbgetRsNzbFileParser* nzbget_rs_nzbfile_parser_new(const char* file_name);
void nzbget_rs_nzbfile_parser_free(NzbgetRsNzbFileParser* parser);
int nzbget_rs_nzbfile_parser_parse_file(
    NzbgetRsNzbFileParser* parser, const char* par_ignore_ext,
    const NzbgetRsCtype* ct, void* alloc_ctx, NzbgetRsAllocFileIdCb alloc_file_id_cb,
    void (*on_parsed_subject)(const char*, const char*),
    void (*sort_indices)(const char* const*, size_t*, size_t),
    NzbgetRsNzbFileSummary* out_summary);
int nzbget_rs_nzbfile_parser_get_message(
    const NzbgetRsNzbFileParser* parser, size_t index,
    int* out_kind, const char** out_text);
int nzbget_rs_nzbfile_parser_get_file(
    const NzbgetRsNzbFileParser* parser, size_t index,
    NzbgetRsNzbKeptFileView* out_view);



int nzbget_rs_dupecoord_files_on_disk(
    const char* final_dir, const char* dest_dir, const char* ext_cleanup_disk);
NzbgetRsBuf nzbget_rs_dupecoord_no_backup_reason(
    int fails_over_flag, int dupe_mode, const NzbgetRsDupeHistoryCandidate* history,
    const char* const* queued_filenames, size_t history_len);


void nzbget_rs_queue_editor_visit_category_ext_diff(
    const char* old_ext, const char* new_ext, void* ctx,
    void (*cb)(void* ctx, const char* param_name, const char* param_val));

int nzbget_rs_twin_sample_indices(
    const size_t* primary_seg_counts, const int64_t* primary_seg_bytes, size_t primary_seg_bytes_len,
    size_t primary_files, const size_t* dupe_seg_counts, const int64_t* dupe_seg_bytes,
    size_t dupe_seg_bytes_len, size_t dupe_files, size_t sample_count,
    size_t* out_file_indices, size_t* out_seg_indices);


size_t nzbget_rs_dupematcher_filter_dupe_sources(
    const char* nzb_name, const char* dest_dir, int64_t expected_size, const char* unrar_cmd,
    const char* const* dupe_dirs, size_t dupe_count, size_t* out_indices,
    void (*on_message)(void* ctx, int kind, const char* msg), void* ctx);

int64_t nzbget_rs_donor_now_ms(void);
void nzbget_rs_donor_stop_all(void);
void nzbget_rs_donor_wait_all(void);
void nzbget_rs_donor_reset(void);
int nzbget_rs_donor_stopping(void);
int nzbget_rs_donor_body_gate_done(void* rust_gate);
int nzbget_rs_donor_body_once(
    void* rust_gate, void* ctx, int (*download_cb)(void* ctx));
void* nzbget_rs_donor_server_new(
    void* cpp_server, int cap,
    int (*exchange_cb)(
        void* cpp_server, size_t count, const char* const* msg_ids, const size_t* msg_lens,
        void* const* rust_gates, const char* const* const* group_ptrs,
        const size_t* const* group_lens, const size_t* group_counts,
        int* out_answers, size_t* out_count, int* out_busy));
void nzbget_rs_donor_server_free(void* handle);
int nzbget_rs_donor_server_cap(const void* handle);
void nzbget_rs_donor_check_posting(
    void* const* server_handles, size_t server_count,
    void* keepalive_ptr, void (*keepalive_drop)(void* keepalive_ptr),
    const char* const* ids_ptr, const size_t* ids_lens, size_t ids_count,
    const char* const* group_ptrs, const size_t* group_lens, size_t group_count,
    int percent, int minimum, int maximum, int body_percent, int max_body, uint32_t seed,
    int probe, int budget_ms, int64_t deadline_ms, int full,
    int* out_checked, int* out_present, int* out_missing, int* out_error,
    int* out_body_checked, int* out_body_bad);
void nzbget_rs_donor_check_postings(
    void* const* server_handles, size_t server_count,
    void* keepalive_ptr, void (*keepalive_drop)(void* keepalive_ptr),
    size_t posting_count, const char* const* key_ptrs, const size_t* key_lens,
    const char* const* const* ids_ptrs, const size_t* const* ids_lens, const size_t* ids_counts,
    const char* const* const* group_ptrs, const size_t* const* group_lens, const size_t* group_counts,
    int percent, int minimum, int maximum, int body_percent, int max_body, uint32_t seed,
    int probe, int budget_ms, int64_t deadline_ms, int full, int concurrency,
    void* emit_ctx,
    void (*emit_cb)(
        void* emit_ctx, const char* key_ptr, size_t key_len,
        int checked, int present, int missing, int error, int body_checked, int body_bad));

void* nzbget_rs_nzb_fetcher_new(void);
void nzbget_rs_nzb_fetcher_free(void* handle);
void nzbget_rs_nzb_fetcher_set_state_path(void* handle, const char* path, size_t path_len);
void nzbget_rs_nzb_fetcher_load(void* handle, int64_t now);
int nzbget_rs_nzb_fetcher_fetch(
    void* handle, const char* indexer, size_t indexer_len,
    const char* title, size_t title_len, const char* link, size_t link_len,
    int64_t size, int grabs, int64_t date, int64_t deadline, int retries,
    void* budget_handle, void* ctx,
    int64_t (*now_cb)(void* ctx), int (*stopped_cb)(void* ctx),
    void (*sleep_100ms_cb)(void* ctx),
    int (*http_fetch_cb)(
        void* ctx, const char* url, size_t url_len, const char* label, size_t label_len,
        size_t max_bytes, int64_t deadline_ms, int* out_status,
        const char** out_body_ptr, size_t* out_body_len),
    void (*log_info_cb)(void* ctx, const char* msg),
    void (*log_warn_cb)(void* ctx, const char* msg),
    NzbgetRsBuf* out_data,
    RsNzbSummaryScalars* out_scalars,
    void* summary_ctx,
    void (*on_strings)(
        void* ctx, const uint8_t* poster_ptr, size_t poster_len,
        const uint8_t* main_name_ptr, size_t main_name_len,
        const uint8_t* par_name_ptr, size_t par_name_len,
        const uint8_t* par_id_ptr, size_t par_id_len, int64_t par_bytes),
    void (*on_slice)(
        void* ctx, int kind, const uint8_t* ptr1, size_t len1,
        const uint8_t* ptr2, size_t len2, int64_t bytes),
    void (*on_recovery_bytes)(void* ctx, const uint64_t* ptr, size_t len));


int nzbget_rs_dupe_identity_fetch_article(
    const char* msg_id, size_t msg_id_len, int64_t deadline, void* ctx,
    int64_t (*now_cb)(void* ctx), int (*stopped_cb)(void* ctx),
    int (*fetch_cb)(
        void* ctx, const char* bracketed_id, size_t bracketed_len,
        int* out_success, int64_t* out_offset, int64_t* out_file_size),
    void (*stop_fetch_cb)(void* ctx));
int nzbget_rs_dupe_identity_read(
    int64_t data_bytes, const char* const* head_names, const size_t* head_name_lens,
    const char* const* head_msg_ids, const size_t* head_msg_id_lens,
    const int64_t* head_bytes, size_t head_count, int64_t deadline, void* ctx,
    int64_t (*now_cb)(void* ctx), int (*stopped_cb)(void* ctx),
    void (*fetch_head_cb)(
        void* ctx, const char* msg_id, size_t msg_id_len, int64_t dl,
        int* out_success, int64_t* out_file_size,
        const char** out_data_ptr, size_t* out_data_len),
    NzbgetRsBuf* out_name, NzbgetRsBuf* out_ext, uint64_t* out_bytes);

void* nzbget_rs_regex_new(const char* pattern, int match_buf_size);
void nzbget_rs_regex_free(void* handle);
int nzbget_rs_regex_is_valid(const void* handle);
int nzbget_rs_regex_match(void* handle, const char* str_ptr);

void* nzbget_rs_dir_browser_new(const char* path, int snapshot);
const char* nzbget_rs_dir_browser_next(void* handle);
void nzbget_rs_dir_browser_free(void* handle);

void* nzbget_rs_article_batch_fetcher_new(
    void* ctx,
    int (*fetch_cb)(
        void* ctx, size_t worker_idx, const char* msg_id,
        const char* const* groups, size_t group_count,
        NzbgetRsBuf* out_data, int64_t* out_offset, int64_t* out_file_size,
        int* out_success, int* out_retry, int* out_transient),
    void (*stop_cb)(void* ctx));
void nzbget_rs_article_batch_fetcher_free(void* handle);
void nzbget_rs_article_batch_fetcher_set_worker_count(void* handle, int worker_count);
void nzbget_rs_article_batch_fetcher_begin(
    void* handle, size_t req_count, void* ctx,
    size_t (*get_req_cb)(
        void* ctx, size_t idx, const char** out_mid_ptr, size_t* out_mid_len),
    void (*get_group_cb)(
        void* ctx, size_t req_idx, size_t grp_idx,
        const char** out_grp_ptr, size_t* out_grp_len));
int nzbget_rs_article_batch_fetcher_next(
    void* handle, NzbgetRsBuf* out_data, int64_t* out_offset, int64_t* out_file_size,
    int* out_success, int* out_retry, int* out_transient);
void nzbget_rs_article_batch_fetcher_cancel_remaining(void* handle);
void nzbget_rs_article_batch_fetcher_stop(void* handle);
NzbgetRsBuf nzbget_rs_alloc_buf(const char* data, size_t len);

NzbgetRsBuf nzbget_rs_ext_mgr_load_extensions(
    const char* script_dir, const char* script_order, char path_sep, void* ctx,
    void (*on_error)(void* ctx, const char* msg),
    void (*on_extension)(
        void* ctx,
        const char* entry_ptr, size_t entry_len,
        const char* loc_ptr, size_t loc_len,
        const char* root_ptr, size_t root_len,
        const char* name_ptr, size_t name_len,
        const char* disp_ptr, size_t disp_len,
        const char* qevents_ptr, size_t qevents_len,
        const char* task_time_ptr, size_t task_time_len,
        int post_script, int scan_script, int queue_script, int sched_script, int feed_script,
        const char* json_ptr, size_t json_len,
        const char* xml_ptr, size_t xml_len));
NzbgetRsBuf nzbget_rs_ext_mgr_resolve_extension_path(
    const char* ext_loc_ptr, size_t ext_loc_len,
    const char* ext_entry_ptr, size_t ext_entry_len,
    const char* const* all_locs_ptrs, const size_t* all_locs_lens, size_t all_locs_count);
NzbgetRsBuf nzbget_rs_system_info_format(
    int mode,
    const char* uts_sysname,
    const char* uts_release,
    const char* uts_machine,
    const char* ssl_lib_name,
    const char* ssl_raw_version,
    int ssl_extract_after_space,
    const char* pub_ip_ptr, size_t pub_ip_len,
    const char* priv_ip_ptr, size_t priv_ip_len);

int nzbget_rs_server_pool_select_connection(
    int start_level,
    const int32_t* levels_ptr, size_t levels_len,
    const int32_t* srv_ids, const int32_t* srv_groups, const int32_t* srv_norm_levels,
    const uint8_t* srv_active, const uint8_t* srv_optional, const uint8_t* srv_blocked,
    size_t srv_count,
    const size_t* conn_srv_indices, const uint8_t* conn_in_use, const uint8_t* conn_connected,
    size_t conn_count,
    int has_want, int32_t want_id, int32_t want_group,
    const int32_t* ign_ids, const int32_t* ign_groups, const int32_t* ign_norm_levels,
    size_t ign_count,
    int rand_val,
    size_t* out_unblock_indices, size_t* out_unblock_count,
    size_t* out_conn_index, int32_t* out_chosen_level);

int nzbget_rs_script_config_read_cipher_snapshot(
    const char* filename, NzbgetRsBuf* out_bytes);
int nzbget_rs_script_config_commit_cipher_patch(
    const char* filename,
    const char* expected_ptr, size_t expected_len,
    const char* replacement_ptr, size_t replacement_len,
    void* ctx,
    int (*cancel_cb)(void*),
    void (*on_dir_sync_fail_cb)(void*));

typedef struct NzbgetRsParsedOptions {
    int32_t download_rate;
    int32_t article_timeout;
    int32_t article_read_chunk_size;
    int32_t url_timeout;
    int32_t remote_timeout;
    int32_t article_retries;
    int32_t article_interval;
    int32_t url_retries;
    int32_t url_interval;
    int32_t control_port;
    int32_t secure_port;
    int32_t url_connections;
    int32_t log_buffer;
    int32_t rotate_log;
    int32_t umask;
    int32_t update_interval;
    int32_t write_buffer;
    int32_t nzb_dir_interval;
    int32_t nzb_dir_file_age;
    int32_t nzb_dir_archive_scan;
    int32_t nzb_dir_archive_action;
    int32_t disk_space;
    int32_t par_time_limit;
    int32_t keep_history;
    int32_t feed_history;
    int32_t time_correction;
    int32_t propagation_delay;
    int32_t article_cache;
    int32_t memory_pressure_interval;
    int32_t event_interval;
    int32_t par_buffer;
    int32_t par_threads;
    int32_t monthly_quota;
    int32_t quota_start_day;
    int32_t daily_quota;
    int32_t system_health_check;
    int32_t nzb_log;
    int32_t append_category_dir;
    int32_t continue_partial;
    int32_t flush_queue;
    int32_t dupe_check;
    int32_t par_repair;
    int32_t par_quick;
    int32_t par_rename;
    int32_t rar_rename;
    int32_t direct_rename;
    int32_t hard_linking;
    int32_t curses_nzb_name;
    int32_t curses_time;
    int32_t curses_group;
    int32_t crc_check;
    int32_t direct_write;
    int32_t raw_article;
    int32_t skip_write;
    int32_t crash_trace;
    int32_t crash_dump;
    int32_t par_pause_queue;
    int32_t script_pause_queue;
    int32_t nzb_cleanup_disk;
    int32_t form_auth;
    int32_t json_p;
    int32_t secure_control;
    int32_t unpack;
    int32_t direct_unpack;
    int32_t use_temp_unpack_dir;
    int32_t unpack_cleanup_disk;
    int32_t unpack_pause_queue;
    int32_t unpack_mutex_enforced;
    int32_t url_force;
    int32_t cert_check;
    int32_t reorder_files;
    int32_t post_download_rename;
    int32_t rename_after_unpack;
    int32_t output_mode;
    int32_t par_check;
    int32_t par_scan;
    int32_t dupe_article_fallback;
    int32_t dupe_stream_decompress;
    int32_t dupe_stream_timeout;
    int32_t dupe_search;
    int32_t dupe_search_dry_run;
    int32_t dupe_search_delay;
    int32_t dupe_search_max_donors;
    int32_t dupe_health_percent;
    int32_t dupe_health_min;
    int32_t dupe_health_max;
    int32_t dupe_body_checks;
    int32_t dupe_health_budget;
    int32_t dupe_min_alive;
    int32_t dupe_fast_donors;
    int32_t post_strategy;
    int32_t file_naming;
    int32_t health_check;
    int32_t info_target;
    int32_t warning_target;
    int32_t error_target;
    int32_t debug_target;
    int32_t detail_target;
    int32_t write_log;
    int32_t tls;
} NzbgetRsParsedOptions;

typedef struct NzbgetRsOptionsHost {
    void* ctx;
    int (*find_entry)(
        void* ctx, const char* name,
        const char** out_val, const char** out_def_val, int* out_line_no);
    void (*set_entry_raw_value)(void* ctx, const char* name, const char* value);
    void (*set_option)(void* ctx, const char* name, const char* value);
    void (*set_config_line)(void* ctx, int line);
    int (*get_config_line)(void* ctx);
    void (*config_error)(void* ctx, const char* msg);
    void (*config_warn)(void* ctx, const char* msg);
    int (*file_exists)(void* ctx, const char* path);
    int (*ensure_directory)(void* ctx, const char* dir, char* err_buf, size_t err_cap);
    void (*add_news_server)(
        void* ctx, int id, int active, const char* name, const char* host,
        int port, int ip_version, const char* user, const char* pass,
        int join_group, int tls, const char* cipher, int max_connections,
        int retention, int level, int group, int optional, uint32_t cert_verif_level);
    void (*add_feed)(
        void* ctx, int id, const char* name, const char* url, int interval,
        const char* filter, int backlog, int pause_nzb, const char* category,
        int category_source, int priority, const char* extensions, uint32_t cert_verif_level);
    void (*add_task)(
        void* ctx, int id, int hours, int minutes, int week_days_bits,
        int command, const char* param);
    void (*add_category)(
        void* ctx, const char* name, const char* dest_dir, int unpack,
        const char* extensions, const char* const* aliases, size_t aliases_len);
} NzbgetRsOptionsHost;

void nzbget_rs_options_init_defaults(
    int disable_curses,
    void (*set_opt)(void* ctx, const char* name, const char* value),
    void* ctx);
NzbgetRsBuf nzbget_rs_options_expand_variables(
    const char* initial_value,
    const char* (*get_opt)(void* ctx, const char* name),
    void* ctx);
int nzbget_rs_options_split_and_convert(
    const char* option, NzbgetRsBuf* out_name, NzbgetRsBuf* out_value);
NzbgetRsBuf nzbget_rs_options_resolve_tool_path(const char* value, size_t len);
int nzbget_rs_options_category_alias_list_matches(
    const char* const* aliases, size_t aliases_len, const char* name, int signed_char);
NzbgetRsBuf nzbget_rs_options_check_dir(
    const NzbgetRsOptionsHost* host, int no_disk_access,
    const char* option_name, const char* parent_dir,
    int allow_empty, int create, int* out_has_dir);
NzbgetRsBuf nzbget_rs_options_check_script_dir(
    const NzbgetRsOptionsHost* host, int no_disk_access,
    const char* main_dir, int* out_has_dir);
void nzbget_rs_options_init_scalars(
    const NzbgetRsOptionsHost* host,
    NzbgetRsParsedOptions* out_scalars,
    int* out_has_mem_path, NzbgetRsBuf* out_mem_path,
    int* out_has_hl_ext, NzbgetRsBuf* out_hl_ext,
    int* out_has_dupe_url, NzbgetRsBuf* out_dupe_url,
    int* out_has_dupe_key, NzbgetRsBuf* out_dupe_key,
    int* out_warn_dupe_no_url);
NzbgetRsBuf nzbget_rs_options_check_options(
    const NzbgetRsOptionsHost* host,
    NzbgetRsParsedOptions* scalars,
    int no_disk_access, int is_32bit,
    int disable_parcheck, int disable_curses, int disable_tls,
    int have_x509_check_host, int cert_store_empty,
    const char* web_dir, const char* config_template, const char* unpack_pass_file,
    int* out_ct_changed);
void nzbget_rs_options_init_servers(
    const NzbgetRsOptionsHost* host, int disable_tls, NzbgetRsParsedOptions* scalars);
void nzbget_rs_options_init_categories(
    const NzbgetRsOptionsHost* host, int no_disk_access, const char* dest_dir);
void nzbget_rs_options_init_feeds(const NzbgetRsOptionsHost* host);
void nzbget_rs_options_init_scheduler(const NzbgetRsOptionsHost* host);
void nzbget_rs_options_create_scheduler_task(
    const NzbgetRsOptionsHost* host, int id, const char* time,
    const char* week_days, int command, const char* param);

int nzbget_rs_unpack_can_extract_with_zipfast(const char* archive_path);
int nzbget_rs_unpack_extract_with_zipfast(
    const char* archive_path, const char* output_dir, const char* password,
    int overwrite_mode, int cleanup_disk,
    void (*log_cb)(void* ctx, int kind, const char* msg), void* log_ctx,
    int (*is_stopped_cb)(void* ctx));

void nzbget_rs_unpack_controller_check_archive_files(
    const char* dest_dir,
    int use_temp_unpack_dir,
    const char* const* downloaded_files,
    size_t downloaded_len,
    const char* const* extracted_archives,
    size_t extracted_len,
    const char* unpack_ignore_ext,
    int* out_has_rar_files,
    int* out_has_not_unpacked_rar_files,
    int* out_has_renamed_archive_files,
    int* out_has_seven_zip_files,
    int* out_has_seven_zip_multi_files,
    int* out_has_splitted_files);
int nzbget_rs_unpack_controller_extract_archives(
    const char* dest_dir,
    const char* unpack_dir,
    const char* password,
    int is_seven_zip,
    int multi_volumes,
    int use_temp_unpack_dir,
    int cleanup_disk,
    const char* const* downloaded_files,
    size_t downloaded_len,
    void* ctx,
    void (*reset_all_ok_cb)(void* ctx),
    int (*has_all_ok_cb)(void* ctx),
    void (*log_cb)(void* ctx, int kind, const char* msg),
    int (*is_stopped_cb)(void* ctx));
int nzbget_rs_unpack_controller_join_splitted_files(
    const char* dest_dir,
    const char* unpack_dir,
    int par_success,
    int write_buffer_kb,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*progress_cb)(void* ctx, const char* label, int stage_progress),
    void (*add_joined_cb)(void* ctx, const char* piece_name));
int nzbget_rs_unpack_controller_cleanup(
    const char* dest_dir,
    const char* final_dir,
    const char* unpack_dir,
    int unpack_ok,
    int use_temp_unpack_dir,
    int final_dir_created,
    int unpack_cleanup_disk,
    int inter_dir,
    int has_splitted_files,
    uint32_t umask,
    const char* const* downloaded_files,
    size_t downloaded_len,
    const char* const* joined_files,
    size_t joined_len,
    int* out_cleaned_up_disk,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg));
int nzbget_rs_direct_unpack_is_main_archive(const char* filename);
void nzbget_rs_direct_unpack_find_archive_files(
    const char* dest_dir,
    void* ctx,
    void (*add_archive_cb)(void* ctx, const char* archive_name));
void nzbget_rs_direct_unpack_cleanup(
    int use_temp_unpack_dir,
    const char* unpack_dir,
    int final_dir_created,
    const char* final_dir,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg));

void nzbget_rs_stream_repair_process_donor(
    const char* dest_dir,
    int live_mode,
    const char* const* member_names,
    size_t member_count,
    const char* const* target_filenames,
    const int64_t* target_encoded_sizes,
    const int64_t* target_decoded_sizes,
    const int* target_article_counts,
    const int* target_positional_ranks,
    const int* target_positional_windows,
    const uint64_t* target_steps_hashes,
    const uint8_t* target_patch_eligible,
    const size_t* target_hole_counts,
    const NzbgetRsByteRange* target_holes_flat,
    size_t target_count,
    const char* donor_info_name,
    const char* const* donor_file_names,
    const int64_t* donor_file_sizes,
    const uint8_t* donor_file_is_par,
    const uint64_t* donor_file_steps_hashes,
    const size_t* donor_article_counts,
    const int32_t* donor_article_sizes_flat,
    size_t donor_file_count,
    void* ctx,
    int (*is_stopped_cb)(void* ctx),
    const char* (*refresh_live_name_cb)(void* ctx, size_t target_index),
    void (*begin_member_batch_cb)(void* ctx, int32_t member_index, const int32_t* part_indices, size_t count),
    int (*next_article_cb)(
        void* ctx,
        int* out_success,
        int64_t* out_offset,
        int64_t* out_file_size,
        const uint8_t** out_data,
        size_t* out_data_len),
    void (*cancel_batch_cb)(void* ctx),
    void (*add_progress_cb)(void* ctx, int64_t bytes),
    void (*add_recovered_cb)(void* ctx, int64_t bytes),
    void (*sync_target_cb)(
        void* ctx,
        size_t target_index,
        const char* filename,
        int64_t decoded_file_size,
        int recreated,
        const NzbgetRsByteRange* holes,
        size_t hole_count),
    void (*emit_message_cb)(void* ctx, int32_t kind, const char* text));

typedef struct NzbgetRsCrossPackSession NzbgetRsCrossPackSession;

NzbgetRsCrossPackSession* nzbget_rs_cross_pack_session_new(
    const char* dest_dir,
    const char* const* member_names,
    size_t member_count,
    const char* const* target_filenames,
    const int64_t* target_decoded_sizes,
    const uint8_t* target_patch_eligible,
    const size_t* target_hole_counts,
    const NzbgetRsByteRange* target_holes_flat,
    size_t target_count,
    const char* target_password,
    void* ctx,
    int (*is_stopped_cb)(void* ctx),
    void (*emit_message_cb)(void* ctx, int32_t kind, const char* text),
    void (*emit_debug_cb)(void* ctx, const char* text));

void nzbget_rs_cross_pack_session_free(NzbgetRsCrossPackSession* session);

int nzbget_rs_cross_pack_session_has_inner_holes(const NzbgetRsCrossPackSession* session);

void nzbget_rs_cross_pack_session_process_donor(
    NzbgetRsCrossPackSession* session,
    const char* donor_queued_filename,
    const char* donor_info_name,
    const char* donor_password,
    const char* const* donor_file_names,
    const int64_t* donor_file_sizes,
    const size_t* donor_article_counts,
    const int32_t* donor_article_sizes_flat,
    size_t donor_file_count,
    int allow_decompress,
    void* ctx,
    int (*is_stopped_cb)(void* ctx),
    int (*fetch_part_cb)(
        void* ctx,
        int32_t member_index,
        int32_t part_index,
        int* out_success,
        int64_t* out_offset,
        int64_t* out_file_size,
        const uint8_t** out_data,
        size_t* out_data_len),
    void (*begin_prefetch_cb)(
        void* ctx,
        int32_t member_index,
        const int32_t* part_indices,
        size_t count),
    int (*next_batch_cb)(
        void* ctx,
        int* out_success,
        int64_t* out_offset,
        int64_t* out_file_size,
        const uint8_t** out_data,
        size_t* out_data_len),
    void (*cancel_batch_cb)(void* ctx),
    void (*begin_materialize_member_cb)(void* ctx, int32_t member_index),
    void (*set_progress_cb)(void* ctx, const char* label, int32_t stage_progress),
    void (*set_materializing_cb)(void* ctx, int32_t delta),
    void (*add_recovered_cb)(void* ctx, int64_t bytes, int32_t holes),
    void (*sync_target_holes_cb)(
        void* ctx,
        const size_t* updated_counts,
        const NzbgetRsByteRange* updated_flat,
        size_t target_count),
    void (*emit_message_cb)(void* ctx, int32_t kind, const char* text),
    void (*emit_extractor_log_cb)(void* ctx, int32_t kind, const char* text),
    void (*emit_debug_cb)(void* ctx, const char* text));


uint32_t nzbget_rs_crc32_append(const uint8_t* block, uint32_t len, uint32_t init);

void nzbget_rs_par_repairer_delete_leftovers(
    const NzbgetRsParRepairer* handle,
    void* ctx,
    void (*log_cb)(void* ctx, const char* msg));
int nzbget_rs_par_repairer_check_empty_files(
    const NzbgetRsParRepairer* handle,
    const char* const* processed_files,
    size_t processed_len,
    void* ctx,
    void (*log_cb)(void* ctx, const char* msg));
int nzbget_rs_par_repairer_target_file_exists_by_basename(
    const NzbgetRsParRepairer* handle,
    const char* file_name);
void nzbget_rs_par_repairer_for_each_target_verification(
    const NzbgetRsParRepairer* handle,
    int set_succeeded,
    void* ctx,
    void (*cb)(void* ctx, const char* target_path, int keep_verified));
void nzbget_rs_par_repairer_discover_splitted_fragments(
    const NzbgetRsParRepairer* handle,
    const char* dest_dir,
    const char* const* processed_files,
    size_t processed_len,
    void* ctx,
    int (*is_parred_cb)(void* ctx, const char* filename),
    const char* (*find_origname_cb)(void* ctx, const char* current),
    void (*detail_cb)(void* ctx, const char* msg),
    void (*add_fragment_cb)(void* ctx, const char* full_path));
void nzbget_rs_par_repairer_find_and_sort_extra_files(
    const char* directory,
    int external_dir,
    const char* par_filename,
    const char* const* processed_files,
    size_t processed_len,
    void* ctx,
    int (*is_parred_cb)(void* ctx, const char* filename),
    void (*add_extra_cb)(void* ctx, const char* full_path));

int nzbget_rs_system_info_fetch_network(
    const char* host, int use_tls, int include_gzip, const char* version_rev,
    void* ctx,
    int (*tls_cb)(
        void* ctx, int fd, const char* host, const char* req, size_t req_len,
        NzbgetRsBuf* out_resp),
    NzbgetRsBuf* out_public_ip, NzbgetRsBuf* out_private_ip, NzbgetRsBuf* out_err);
NzbgetRsBuf nzbget_rs_maintenance_read_package_info_str(
    const char* web_dir, char sep, const char* key,
    int* out_err_kind, NzbgetRsBuf* out_file_name);
int nzbget_rs_maintenance_prepare_signature_inputs(
    const char* in_filename, const char* sig_filename, const char* pub_key_filename,
    uint8_t* out_in_hash, uint8_t* out_signature, NzbgetRsBuf* out_pub_key);

int nzbget_rs_unpack_controller_try_password_file(
    const char* path, void* ctx,
    int (*should_continue_cb)(void*),
    void (*try_password_cb)(void*, const char*));

NzbgetRsBuf nzbget_rs_log_rotate_log_file(
    const char* log_file, char sep, int64_t cur_time, int rotate_log,
    void* ctx, void (*on_info_cb)(void*, const char*));
NzbgetRsBuf nzbget_rs_options_find_default_config_file(
    const char* app_dir, char sep, int* out_found);
int nzbget_rs_options_load_config_file(
    const char* path, void* ctx,
    void (*on_line_cb)(void*, int, const char*));

NzbgetRsBuf nzbget_rs_nzb_format_name(
    int op, const char* s1, const char* s2, const char* s3,
    const char* s4, const char* s5, int flag, int nzb_id);
int nzbget_rs_nzb_calc_health(
    int is_critical, int64_t size, int64_t par_size,
    int64_t current_failed_size, int64_t par_current_failed_size, int allow_estimation);
int nzbget_rs_nzb_is_dupe_success(
    int mark_status, int delete_status, int par_status, int unpack_status,
    int url_status, int health, int critical_health_est, int critical_health_exact,
    int health_check_dupe);
const char* nzbget_rs_nzb_make_text_status(
    int kind, int mark_status, int delete_status, int par_status, int unpack_status,
    int move_status, int url_status, int script_status, int health, int critical_health);
int nzbget_rs_nzb_completed_same_filename(const char* a, const char* b);
void nzbget_rs_nzb_calc_active_downloads_sec(
    int old_active, int new_active, int is_nzb_kind, int64_t cur_time,
    int* inout_download_sec, int64_t* inout_download_start_time,
    int* inout_download_start_sec, int* out_changed);
void nzbget_rs_nzb_update_completed_stats(
    int64_t file_success_size, int64_t file_failed_size,
    int file_failed_articles, int file_success_articles,
    int file_extra_priority, int file_par_file, int file_paused,
    int64_t* inout_success_size, int64_t* inout_failed_size,
    int* inout_failed_articles, int* inout_success_articles,
    int* inout_extra_priority, int64_t* inout_par_success_size,
    int64_t* inout_par_failed_size, int* inout_remaining_par_count,
    int* inout_paused_file_count);
void nzbget_rs_nzb_update_deleted_stats(
    int64_t file_size, int64_t file_success_size, int64_t file_failed_size,
    int64_t file_missed_size, int file_total_articles, int file_success_articles,
    int file_failed_articles, int file_missed_articles, int64_t file_remaining_size,
    int file_extra_priority, int file_par_file, int file_paused,
    int* inout_file_count, int64_t* inout_size, int64_t* inout_cur_success_size,
    int64_t* inout_failed_size, int* inout_failed_articles, int64_t* inout_cur_failed_size,
    int* inout_total_articles, int* inout_cur_success_articles, int* inout_cur_failed_articles,
    int64_t* inout_remaining_size, int* inout_extra_priority, int* inout_remaining_par_count,
    int64_t* inout_par_size, int64_t* inout_par_cur_success_size, int64_t* inout_par_failed_size,
    int64_t* inout_par_cur_failed_size, int* inout_paused_file_count, int64_t* inout_paused_size);

NzbgetRsBuf nzbget_rs_util_find_executor_program(
    const char* filename, const char* custom_path, int* out_found);
void nzbget_rs_util_format_time(int64_t time_sec, char* buffer, int bufsize);

typedef struct RsDownloadCurrentStatsAcc {
    int64_t remaining_size;
    int64_t current_success_size;
    int64_t current_failed_size;
    int64_t paused_size;
    int64_t par_current_success_size;
    int64_t par_current_failed_size;
    int32_t current_success_articles;
    int32_t current_failed_articles;
    int32_t extra_priority;
    int32_t paused_file_count;
    int32_t remaining_par_count;
    int32_t _pad;
} RsDownloadCurrentStatsAcc;

NzbgetRsBuf nzbget_rs_fs_string_op(int op, const char* path, int* out_ok);
int64_t nzbget_rs_fs_op_with_err(
    int op, const char* p1, const char* p2, int64_t val, NzbgetRsBuf* out_errmsg);
int nzbget_rs_fs_load_file_into_buffer(
    const char* filename, int add_trailing_null, void* ctx,
    uint8_t* (*reserve_cb)(void*, int));
int nzbget_rs_fs_get_disk_state(
    const char* path, int64_t* out_avail, int64_t* out_total);
char* nzbget_rs_tokenizer_next(
    char* data_str, const char* separators, char** inout_save_ptr, int* inout_working);
void nzbget_rs_nzb_update_current_stats_step(
    RsDownloadCurrentStatsAcc* acc, int64_t file_remaining_size,
    int file_success_articles, int file_failed_articles,
    int64_t file_success_size, int64_t file_failed_size,
    int file_extra_priority, int file_paused, int file_par_file);
void nzbget_rs_nzb_calc_min_max_time(
    const int64_t* times, size_t count, int64_t* out_min, int64_t* out_max);
void nzbget_rs_server_stat_apply_op(
    int stat_op, int delta_success, int delta_failed,
    int* inout_success, int* inout_failed);
int nzbget_rs_script_calc_total_status(const int* statuses, size_t count);
size_t nzbget_rs_stream_repair_compute_holes(
    const NzbgetRsByteRange* covered, size_t count, int64_t file_size,
    NzbgetRsByteRange* out_holes, size_t max_holes);
int nzbget_rs_queue_coord_is_data_filename(const char* filename);

typedef struct NzbgetRsArticleGeom {
    int32_t part_number;
    int32_t status;
    int64_t segment_offset;
    int32_t segment_size;
    int32_t fallback_round;
} NzbgetRsArticleGeom;

typedef struct NzbgetRsDonorScoreEntry {
    int score;
    double alive;
    int twin;
    int64_t bytes;
    int grabs;
} NzbgetRsDonorScoreEntry;

int nzbget_rs_dupe_fallback_is_par_file(
    int par_file, int first_content, int fc_par2, int filename_confirmed, const char* filename);
int nzbget_rs_dupe_fallback_par_cannot_cover(
    int64_t par_size, int64_t par_failed_size, int64_t current_failed_size,
    int64_t current_success_size, int64_t par_current_success_size, int64_t total_size,
    int current_success_articles, int current_failed_articles, int with_margin);
int nzbget_rs_dupe_fallback_check_try_preconditions(
    int first_content, int fc_par2, int fc_unknown, int part_number, const char* filename,
    int file_deleted, int nzb_deleting, int nzb_parking, int delete_status, int ds_none,
    int dupe_mode, int dm_force);
void nzbget_rs_dupe_fallback_compute_pin_lead_state(
    const int* contributors, size_t count, int cur_file_lead_donor_id,
    int* out_lead_snapshot, int* out_next_lead, int* out_donor_count, int* out_new_file_lead);
int nzbget_rs_dupe_fallback_register_lead_failure(
    int fallback_round, int lead_snapshot, int donor_count, int next_lead,
    int* inout_lead_donor_id, int* inout_lead_failures, int* inout_lead_switches);
void nzbget_rs_dupe_fallback_register_lead_success(
    int fallback_round, int lead_snapshot, int lead_donor_id,
    int64_t expected_offset, int64_t expected_end,
    int* inout_lead_failures, int* inout_lead_switches);
ptrdiff_t nzbget_rs_dupe_fallback_first_untiled_article(
    const NzbgetRsArticleGeom* articles, size_t count, int64_t decoded_size);
int nzbget_rs_dupe_fallback_merge_decoded_file_size(
    int64_t* inout_decoded_file_size, int64_t article_file_size);
int nzbget_rs_dupe_fallback_classify_first_head(const uint8_t* head, size_t len);
int nzbget_rs_script_parse_nzb_cmd(
    int scope, const char* msg_text, int event, int qe_nzb_downloaded,
    size_t* out_key_off, size_t* out_key_len, size_t* out_val_off, int* out_int_val);
int nzbget_rs_post_script_analyse_exit_code(
    int exit_code, int par_status, int ps_skipped, int parcheck_enabled,
    int sr_none, int sr_failure, int sr_success, int* out_msg_action, int* out_req_par_check);
void nzbget_rs_nzb_summary_fingerprint(
    const char* const* ids_ptr, const size_t* ids_lens, size_t count, char* out_hex16);
NzbgetRsBuf nzbget_rs_newznab_mask(const char* text, size_t len);
int nzbget_rs_donor_score_calc(
    int op, int pick_score, double alive, int twin, const int* used_ptr, size_t used_len);
void nzbget_rs_donor_score_rerank(
    const NzbgetRsDonorScoreEntry* entries, size_t count, int64_t pick_bytes, int* out_scores);
void nzbget_rs_dupesearch_parse_pending_meta(
    const char* meta_ptr, size_t meta_len,
    size_t* out_title_off, size_t* out_title_len,
    size_t* out_indexer_off, size_t* out_indexer_len,
    int* out_grabs, int64_t* out_size, int64_t* out_date);
int nzbget_rs_dupesearch_parse_state_line(
    const char* line_ptr, size_t line_len, int64_t now, int64_t ttl_sec,
    size_t* out_key_off, size_t* out_key_len, int* out_score, int64_t* out_time,
    size_t* out_title_off, size_t* out_title_len, int* out_retryable);

typedef struct NzbgetRsServerMatchItem {
    int id;
    int port;
    const char* name;
    const char* host;
    const char* user;
} NzbgetRsServerMatchItem;

typedef struct NzbgetRsDiskNzbExport {
    int hist_id;
    int hist_kind;
    int hist_time;
    int changed;
    NzbgetRsDiskNzbInfoInput info;
    const NzbgetRsDiskCompletedFile* completed;
    size_t completed_len;
    const NzbgetRsDiskParam* params;
    size_t params_len;
    const NzbgetRsDiskScriptStatus* scripts;
    size_t scripts_len;
    const NzbgetRsDiskServerStat* stats;
    size_t stats_len;
    const NzbgetRsDiskFileStub* files;
    size_t files_len;
    int dupe_recovered_articles;
    int64_t dupe_recovered_bytes;
    int dupe_recovered_holes;
    const NzbgetRsDiskRepairJob* jobs;
    size_t jobs_len;
    int direct_unpack_status;
    int unpack_tried;
    int par_repaired;
    int last_unpack_status;
    const char* const* archives;
    size_t archives_len;
} NzbgetRsDiskNzbExport;

typedef struct NzbgetRsDiskFileExport {
    int file_id;
    int has_articles;
    int articles_len_count;
    int force_direct_write;
    NzbgetRsDiskFileInfoInput info;
    const char* const* groups;
    size_t groups_len;
    const NzbgetRsDiskArticleInfo* articles;
    size_t articles_len;
    NzbgetRsDiskFileStateInput state;
    const NzbgetRsDiskServerStat* stats;
    size_t stats_len;
    const NzbgetRsDiskArticleState* article_states;
    size_t article_states_len;
} NzbgetRsDiskFileExport;

typedef struct NzbgetRsDiskStateEnv {
    void* ctx;
    const char* queue_dir;
    const char* temp_dir;
    const char* line_ending;
    const char* time_str;
    const char* text;
    int sep;
    int has_options;
    int flush_queue;
    int direct_write;
    int continue_partial;
    int pt_finished;
    int ns_running;
    int ns_none;
    int a1;
    int a2;
    int a3;
    int a4;
    int64_t now;
    int* out_flag;
    void (*log_cb)(void* ctx, int level, const char* msg);
    int (*ctrl_cb)(void* ctx, int cmd, int a1, int a2, int64_t a3, const char* s1);
    void (*export_nzb_cb)(void* ctx, int source, size_t idx, void* sink_ctx, void (*sink_fn)(void*, const NzbgetRsDiskNzbExport*));
    void (*apply_nzb_cb)(void* ctx, int mode, const NzbgetRsDiskNzbInfoInput* inp, int flags, const NzbgetRsDiskCompletedFile* completed, size_t completed_len, const NzbgetRsDiskParam* params, size_t params_len, const NzbgetRsDiskScriptStatus* scripts, size_t scripts_len, const NzbgetRsDiskServerStat* stats, size_t stats_len, const NzbgetRsDiskFileStub* files, const int* file_times, size_t files_len, const NzbgetRsDiskRepairJob* job);
    void (*export_file_cb)(void* ctx, int mode, void* sink_ctx, void (*sink_fn)(void*, const NzbgetRsDiskFileExport*));
    int (*apply_file_cb)(void* ctx, int mode, int idx_or_flags, int extra_i32, int64_t seg_offset, uint32_t crc, const char* str_arg, const NzbgetRsDiskFileInfoInput* finfo, const NzbgetRsDiskArticleInfo* art_info, const NzbgetRsDiskFileStateInput* fstate, const NzbgetRsDiskServerStat* stats, size_t stats_len);
    void (*aux_cb)(void* ctx, int op, int a1, int a2, uint32_t u1, const char* s1, const char* s2, const NzbgetRsDiskVolumeStatInput* vol_item, void* sink_ctx, void (*sink_fn)(void*, int, int, int, uint32_t, const char*, const NzbgetRsServerMatchItem*, size_t, const NzbgetRsDiskVolumeStatInput*, size_t));
} NzbgetRsDiskStateEnv;

int nzbget_rs_diskstate_exec(int op, const NzbgetRsDiskStateEnv* env);
int nzbget_rs_server_pool_op(
    int op, int64_t a, int64_t b, int c, const int* ptr1, const uint8_t* ptr2, size_t len, int* out_ptr);
int nzbget_rs_server_pool_normalize_levels(
    const int* levels, const uint8_t* active, const int* max_conns, size_t len,
    size_t* out_sorted_indices, int* out_norm_levels);
int nzbget_rs_article_fetch_limit_op(
    int op, int64_t* inout_bytes, int64_t bytes, int64_t begin, int64_t end, int64_t file_size);
int nzbget_rs_statmeter_op(
    int op, int64_t loc_or_cur_time, int64_t a_i64, int64_t b_i64, int64_t c_i64,
    int a_i32, int b_i32, int c_i32, const int64_t* slice_ptr, size_t slice_len,
    int64_t* out_i64_1, int64_t* out_i64_2, int* out_i32);
int nzbget_rs_memory_and_cache_op(
    int op, size_t a, size_t b, int tier, int flag1, int flag2, int flag3, int* inout1, int* inout2);
int nzbget_rs_dupe_probe_op(
    int op, size_t a, int b, int c, int d, const char* str_arg,
    const uint8_t* in_a, const int* in_b, const int* in_c, size_t* out_ptr);
NzbgetRsBuf nzbget_rs_dupe_probe_eval_outcome(
    int op, int existing, int sampled, int missing_servers, int active_servers,
    int unreachable_servers, int reached_servers, int finished, const char* str_arg,
    int* out_outcome);
int nzbget_rs_history_coord_op(
    int op, int a, int b, int c, int d, int e, int64_t f, int64_t g, const char* str_arg);
void nzbget_rs_history_reset_articles_batch(
    int all_failed, int reset_failed, int is_par_file, int partial_state,
    int64_t file_size, int64_t file_missed_size, int file_total_articles, int file_missed_articles,
    int64_t* file_failed_size, int* file_failed_articles, int64_t* file_remaining_size,
    int* file_completed_articles, int64_t* file_success_size, int* file_success_articles,
    int64_t* nzb_failed_size, int64_t* nzb_success_size, int* nzb_failed_articles,
    int* nzb_success_articles, int64_t* nzb_par_failed_size, int64_t* nzb_par_success_size,
    int* article_statuses, const int* article_sizes, uint8_t* out_reset_flags, size_t article_count);
int nzbget_rs_queue_editor_op(
    int op, int a, int b, int c, int64_t f1, int64_t f2, const char* s1, const char* s2,
    const int* in1, size_t len1, const int* in2, size_t len2, int* inout1, int* inout2);
uint64_t nzbget_rs_dupe_identity_calc(
    int op, int present, int missing, int error, uint64_t listed_parts, uint64_t declared_parts,
    int64_t total_or_data_bytes, uint64_t article_bytes, uint64_t block_or_min, double alive_in,
    const uint64_t* rec_ptr, size_t rec_len, double* out_f64);
uint32_t nzbget_rs_dupe_watch_item_flags(
    int kind, int in_history, int dupe_success, int deleting, int delete_status,
    int has_post_info, int dupe_mode, int is_donor, int dupe_hint, int has_dupe_fleet);
int nzbget_rs_dupe_watch_is_live_fleet_peer(
    const char* peer_key, size_t peer_len, const char* pick_key, size_t pick_len, uint32_t peer_flags);
NzbgetRsBuf nzbget_rs_dupe_watch_format_state_line(
    int64_t now, int64_t ttl_sec, const char* key, size_t key_len, int score, int64_t time,
    const char* title, size_t title_len, int retryable);
intptr_t nzbget_rs_transient_retries_op(
    void* handle, int op, uintptr_t a, int64_t b, uintptr_t* out_arr, size_t out_cap);
int nzbget_rs_article_dl_op(
    int op, int a, const char* response, int64_t begin_pos, int64_t end_pos,
    int64_t article_file_size, int64_t expected_file_size, int64_t dupe_expected_offset,
    int64_t dupe_expected_end, int64_t* out_offset, int* out_size);
int nzbget_rs_feed_op(
    int op, int a, int b, int c, int d, int64_t t1, int64_t t2,
    const char* s1, const char* s2, const char* s3, const char* s4, int* out_i32);
NzbgetRsBuf nzbget_rs_feed_str_op(
    int op, int a, int b, int c, int d, int e, int f,
    const char* s1, const char* s2, const char* s3, const char* s4, const char* s5, const char* s6,
    NzbgetRsBuf* out_buf2, uint32_t* out_u32);
int nzbget_rs_twin_service_op(
    int op, int a, int b, int c, int d, int64_t f1, int64_t f2,
    const char* s1, const char* s2, const char* s3, const uint8_t* u8_1, const uint8_t* u8_2,
    const int* i32_1, const int64_t* i64_1, size_t len, NzbgetRsBuf* out_buf);
int nzbget_rs_socket_deadline_wait(
    int fd, int read, int write, void* ctx, int64_t (*rem_cb)(void*));
intptr_t nzbget_rs_url_coord_op(
    int op, int a, int b, int c, int64_t t1, int64_t t2, const char* s1,
    const int* arr1, const int* arr2, const int* arr3, const int* arr4, size_t len,
    int* out_i32_1, int* out_i32_2, int* out_i32_3, NzbgetRsBuf* out_buf);
typedef struct NzbgetRsScanner NzbgetRsScanner;
typedef struct NzbgetRsScannerEnv {
    void* ctx; const char* nzb_dir; const char* temp_dir; const char* filename; const char* nzb_name; const char* category; const char* dupe_key; const char* buf_ptr;
    int buf_len; int priority; int dupe_score; int dupe_mode; int auto_category; int add_top; int add_paused; int nzb_dir_interval; int nzb_dir_file_age; int archive_scan; int archive_action; int queue_loaded; int pause_scan; char path_sep;
    void* parameters; void* url_info; int* out_nzb_id; void (*wake_cb)(void*); void (*log_cb)(void*, int, const char*); const char* (*find_category_cb)(void*, const char*, int);
    void (*iter_params_cb)(void*, void*, void (*)(void*, const char*, const char*)); void (*work_params_cb)(void*, int, const char*, const char*);
    void (*scan_script_cb)(void*, const char*, void*, const char*, const char*, const char*, const char*, void*, void (*)(void*, int, const char*), int*, int*, int*, int*, int*);
    int (*parse_nzb_cb)(void*, const char*, const char*, const char**);
    int (*commit_nzb_cb)(void*, int, const char*, const char*, int, const char*, int, const char*, int, int, int, int, void*);
} NzbgetRsScannerEnv;
NzbgetRsScanner* nzbget_rs_scanner_new(void);
void nzbget_rs_scanner_free(NzbgetRsScanner* scanner);
int nzbget_rs_scanner_exec(NzbgetRsScanner* scanner, int op, const NzbgetRsScannerEnv* env, int a1, int a2, int a3);
int nzbget_rs_scheduler_service_op(
    int op, int a, int b, int c, int d, int64_t t1, int64_t t2,
    const char* s1, const char* s2, int* out_i32);
const char* nzbget_rs_scheduler_service_str(int op, int val);

typedef struct RsPostJobCandidate {
    uint8_t has_post_info;
    uint8_t working;
    uint8_t has_queue_script_job;
    uint8_t direct_unpack_running;
    uint8_t has_live_repair_thread;
    int32_t priority;
    uint8_t force_priority;
    uint8_t need_par_check;
    uint8_t need_unpack;
    uint8_t in_active_jobs;
    uint8_t download_completed;
} RsPostJobCandidate;

typedef struct NzbgetRsNzbMergeSplitCounters {
    int32_t file_count;
    int32_t active_downloads;
    uint32_t full_content_hash;
    uint32_t filtered_content_hash;
    int64_t size;
    int64_t remaining_size;
    int32_t paused_file_count;
    int64_t paused_size;
    int64_t success_size;
    int64_t current_success_size;
    int64_t failed_size;
    int64_t current_failed_size;
    int64_t par_size;
    int64_t par_success_size;
    int64_t par_current_success_size;
    int64_t par_failed_size;
    int64_t par_current_failed_size;
    int32_t remaining_par_count;
    int32_t total_articles;
    int32_t success_articles;
    int32_t failed_articles;
    int32_t current_success_articles;
    int32_t current_failed_articles;
    int64_t min_time;
    int64_t max_time;
    int64_t downloaded_size;
    int32_t download_sec;
    int64_t download_start_time;
} NzbgetRsNzbMergeSplitCounters;

typedef struct NzbgetRsSplitFileCounters {
    int64_t size;
    int64_t remaining_size;
    int64_t success_size;
    int64_t failed_size;
    int64_t missed_size;
    int32_t total_articles;
    int32_t missed_articles;
    int32_t success_articles;
    int32_t failed_articles;
    int32_t par_file;
    int32_t paused;
} NzbgetRsSplitFileCounters;

void nzbget_rs_lifecycle_reload_op(int op, int (*state_cb)(int action));
int nzbget_rs_daemon_service_op(
    int op, int a, int b, int c, int d, int64_t t1,
    const char* s1, const char* s2, int* out1, int* out2,
    void* ctx, void (*cb)(void*, const char*));
int nzbget_rs_prepost_op(
    int op, int a, int b, int c, int d, int e, int f, int g, int h, int i,
    int64_t t1, int64_t t2, const char* s1, const char* s2, const char* s3,
    const int* arr, size_t len, int* out1, int* out2, int* out3, int* out4, int* out5, int* out6);
void nzbget_rs_prepost_delete_cleanup(
    int cleanup_disk, int delete_status, int parking, int skip_disk_write, int opt_skip_write,
    const char* dest_dir, const char* final_dir, const char* const* completed_files, size_t completed_count,
    void* ctx, void (*log_cb)(void*, int, const char*, const char*));
int nzbget_rs_prepost_pick_next_job(
    const RsPostJobCandidate* candidates, size_t len,
    int allow_par, int has_active_unpack, int pause_post_process);
int nzbget_rs_queue_coord_op(
    int op, int a, int b, int c, int d, int e, int f, int g, int h, int i, int j,
    int64_t t1, int64_t t2, int64_t t3, int64_t t4,
    const char* s1, const char* s2, const char* const* cstr_arr,
    const int* i32_1, const int* i32_2, const uint8_t* u8_1, const int64_t* i64_1, size_t len,
    int* out1, int* out2, int* out3, int* out_arr);
NzbgetRsBuf nzbget_rs_queue_coord_str_op(
    int op, int a, int b, int c, int64_t t1,
    const char* s1, const char* s2, const char* s3, const int* i32_arr, size_t len,
    int* out_unique, size_t* out_unique_len);
int nzbget_rs_queue_coord_counters_op(
    int op, int a, int b, int c, int d, int64_t t1, int64_t t2,
    const char* s1, const char* s2,
    NzbgetRsNzbMergeSplitCounters* ms1, NzbgetRsNzbMergeSplitCounters* ms2,
    const NzbgetRsSplitFileCounters* sf, int* i32_inout, int64_t* i64_inout);
int nzbget_rs_repair_ctrl_op(
    int op, int a, int b, int c, int d, int e, int f,
    int64_t t1, int64_t t2, int64_t t3, int64_t t4,
    const char* s1, const char* s2, const int* i32_arr, const uint8_t* u8_arr, const int64_t* i64_arr, size_t len,
    int* out1, int* out2, int* out3, int* out4, int* out_arr, int64_t* out_i64_arr);
int nzbget_rs_stream_repair_ctrl_op(
    int op, int a, int b, int c, int d, int e,
    int64_t t1, int64_t t2, int64_t t3, double f1,
    const char* s1, const char* const* cstr_arr, const int64_t* i64_arr, size_t len,
    int* out1, int* out2, int64_t* i64_inout, NzbgetRsBuf* out_buf);
int nzbget_rs_dupesearch_op(
    int op, int a, int b, int c, int d, int e, int f, int g,
    int64_t t1, int64_t t2, double f1,
    const char* s1, const char* s2, const char* s3, const char* s4,
    const char* const* cstr1, const char* const* cstr2,
    const uint8_t* u8_1, const uint8_t* u8_2,
    const int* i32_1, const int* i32_2,
    const int64_t* i64_1, const int64_t* i64_2, size_t len,
    int* out1, int* out2, int* out_arr, int64_t* out_i64, NzbgetRsBuf* out_buf);

typedef struct NzbgetRsConnEnv {
    void* ctx;
    const char* host;
    int port;
    int ip_version;
    int timeout_sec;
    int graceful;
    int force_close;
    int has_deadline;
    const char* const* resolved_addrs;
    size_t resolved_count;
    int* socket_atomic;
    int* status_atomic;
    int* is_listener;
    int* accept_busy;
    int* tls_error;
    size_t* buf_offset;
    size_t* buf_avail;
    int* total_bytes_read;
    uint8_t* read_buf;
    size_t read_buf_cap;
    int (*send_cb)(void* ctx, const char* buf, int len);
    int (*recv_cb)(void* ctx, char* buf, int len);
    void (*error_cb)(void* ctx, const char* msg);
    void (*debug_cb)(void* ctx, const char* msg);
    int (*deadline_wait_cb)(void* ctx, int fd, int read, int write);
    int (*deadline_stopped_cb)(void* ctx);
} NzbgetRsConnEnv;

int nzbget_rs_conn_op(
    int op, const NzbgetRsConnEnv* env, char* buf_ptr, int buf_len,
    const char* arg1, const char* arg2, const char* arg3, int int_arg, int* out_int);

typedef struct NzbgetRsWebDlEnv {
    void* ctx;
    const char* initial_url;
    const char* output_filename;
    const char* info_name;
    const char* version;
    int write_buffer_kb;
    int url_retries;
    int url_interval_sec;
    int retry;
    int force;
    int tls_disabled;
    int gzip_disabled;
    int (*connect_cb)(void* ctx, const char* host, int port, int tls);
    void (*free_conn_cb)(void* ctx);
    void (*write_line_cb)(void* ctx, const char* line);
    int (*read_line_cb)(void* ctx, char* buf, int size, int* out_len);
    int (*read_body_cb)(void* ctx, char* buf, int size);
    int (*is_stopped_cb)(void* ctx);
    int (*is_paused_cb)(void* ctx);
    void (*touch_time_cb)(void* ctx);
    void (*set_status_cb)(void* ctx, int status);
    void (*set_http_status_cb)(void* ctx, int http_status);
    void (*set_orig_filename_cb)(void* ctx, const char* filename);
    void (*set_url_cb)(void* ctx, const char* url);
    void (*on_header_cb)(void* ctx, const char* line);
    void (*on_log_cb)(void* ctx, int level, const char* msg);
} NzbgetRsWebDlEnv;

int nzbget_rs_webdl_execute(int op, int max_redirects, const NzbgetRsWebDlEnv* env);

typedef int (*NzbgetRsXmlRpcActionFn)(
    void* ctx, int op, int a1, int a2, int a3, int a4,
    const char* s1, const char* s2, const char* s3, const char* s4,
    const char* body, size_t body_len,
    const int32_t* ids, size_t ids_len,
    const NzbgetRsRpcParamItem* params, size_t params_len,
    uint32_t* out_u32, double* out_f64, NzbgetRsBuf* out_buf);

typedef void (*NzbgetRsXmlRpcEmitFn)(
    void* emit_ctx, int sub,
    int a1, int a2, int a3, int a4, int a5, int a6,
    int64_t i1, int64_t i2, int64_t i3,
    const char* s1, const char* s2, const char* s3, const char* s4,
    const void* p1, size_t n1, const void* p2, size_t n2,
    const void* p3, size_t n3, const void* p4, size_t n4);

typedef int (*NzbgetRsXmlRpcCollectFn)(
    void* ctx, int kind, int a1, int a2, int a3, int a4, int a5,
    const char* s1, const char* s2, const char* s3,
    const char* s4, const char* s5, const char* s6,
    const void* p1, size_t n1,
    void* emit_ctx, NzbgetRsXmlRpcEmitFn emit_cb);

typedef struct NzbgetRsWebServeEnv {
    void* ctx;
    const char* signature;
    const char* remote_addr;
    const char* version;
    const char* web_dir;
    const char* const* script_dirs;
    size_t script_dirs_count;
    const char* control_username;
    const char* control_password;
    const char* restricted_username;
    const char* restricted_password;
    const char* add_username;
    const char* add_password;
    const char* authorized_ip;
    const char* config_filename;
    const char* shell_override;
    int control_port;
    int form_auth;
    int gzip_enabled;
    int json_p;
    int debug_build;
    int (*read_line_cb)(void* ctx, char* buf, int size);
    int (*recv_cb)(void* ctx, char* buf, int size);
    int (*send_cb)(void* ctx, const char* buf, int size);
    void (*log_cb)(void* ctx, int level, const char* msg);
    NzbgetRsXmlRpcActionFn rpc_action_cb;
    NzbgetRsXmlRpcCollectFn rpc_collect_cb;
} NzbgetRsWebServeEnv;

int nzbget_rs_web_serve(int op, const NzbgetRsWebServeEnv* env);

typedef void (*NzbgetRsBinRpcListPushNzbFn)(
    void* b_ctx, uint32_t id, uint32_t kind, int64_t size, int64_t rem_size, int64_t paused_size,
    uint32_t paused_files, uint32_t rem_pars, uint32_t priority,
    const char* filename, const char* name, const char* dest_dir,
    const char* category, const char* queued_filename);
typedef void (*NzbgetRsBinRpcListPushParamFn)(
    void* b_ctx, uint32_t owner_idx, const char* name, const char* value);
typedef void (*NzbgetRsBinRpcListPushFileFn)(
    void* b_ctx, uint32_t id, uint32_t nzb_idx, int64_t size, int64_t rem_size,
    uint32_t paused, uint32_t fn_confirmed, uint32_t active_dl,
    const char* nzb_name, const char* subject, const char* filename);
typedef void (*NzbgetRsBinRpcListSetStateFn)(
    void* b_ctx, int64_t rem_size, uint32_t dl_rate, int dl_limit, int dl_paused,
    int post_paused, int scan_paused, int thread_cnt, int post_jobs,
    int up_sec, int dn_sec, int stand_by, int64_t all_bytes);
typedef void (*NzbgetRsBinRpcLogBeginFn)(
    void* enc_ctx, int msg_count, uint32_t first_id, int* out_start);
typedef void (*NzbgetRsBinRpcLogPushFn)(
    void* enc_ctx, uint32_t id, uint32_t kind, uint32_t time, const char* text);
typedef void (*NzbgetRsBinRpcPostPushFn)(
    void* enc_ctx, uint32_t id, uint32_t stage, uint32_t stage_prog, uint32_t file_prog,
    uint32_t start_sec, uint32_t stage_sec, const char* filename, const char* name,
    const char* dest_dir, const char* label);
typedef void (*NzbgetRsBinRpcHistPushFn)(
    void* enc_ctx, uint32_t id, uint32_t kind, uint32_t time, int64_t size,
    uint32_t file_cnt, uint32_t par_status, uint32_t script_status,
    uint32_t url_status, const char* name);

typedef struct NzbgetRsBinRpcServeEnv {
    void* ctx;
    const char* remote_addr;
    const char* control_user;
    const char* control_password;
    const char* version;
    int control_port;
    int (*recv_cb)(void* ctx, char* buf, int size);
    int (*send_cb)(void* ctx, const char* buf, int size);
    void (*log_cb)(void* ctx, int kind, const char* msg);
    int (*action_cb)(
        void* ctx, int kind, int a1, int a2, int a3, int a4,
        const char* s1, const char* s2, const char* s3,
        const char* body, int body_len,
        const int32_t* ids, size_t ids_len,
        const char* const* names, size_t names_len);
    void (*collect_list_cb)(
        void* ctx, int want_files, int want_state, void* b_ctx,
        NzbgetRsBinRpcListPushNzbFn push_nzb, NzbgetRsBinRpcListPushParamFn push_param,
        NzbgetRsBinRpcListPushFileFn push_file, NzbgetRsBinRpcListSetStateFn set_state);
    void (*collect_log_cb)(
        void* ctx, int lines, uint32_t id_from, void* enc_ctx,
        NzbgetRsBinRpcLogBeginFn begin_log, NzbgetRsBinRpcLogPushFn push_log);
    void (*collect_post_cb)(
        void* ctx, void* enc_ctx, NzbgetRsBinRpcPostPushFn push_post);
    void (*collect_history_cb)(
        void* ctx, int show_hidden, void* enc_ctx, NzbgetRsBinRpcHistPushFn push_hist);
} NzbgetRsBinRpcServeEnv;

void nzbget_rs_binrpc_serve(const NzbgetRsBinRpcServeEnv* env);

typedef struct NzbgetRsFrontendSummary {
    int64_t current_download_speed;
    int64_t remaining_size;
    int64_t all_bytes;
    int32_t pause_download;
    int32_t download_limit;
    int32_t thread_count;
    int32_t post_job_count;
    int32_t up_time_sec;
    int32_t dn_time_sec;
    int32_t stand_by;
} NzbgetRsFrontendSummary;

typedef struct NzbgetRsFrontendEnv {
    void* ctx;
    int remote_mode;
    int update_interval;
    int use_color;
    int show_nzbname;
    int show_timestamp;
    int group_files;
    int time_correction;
    uint8_t path_sep;
    const char* control_ip;
    int control_port;
    const char* control_username;
    const char* control_password;
    void (*write_stdout_cb)(void* ctx, const char* ptr, size_t len, int do_flush);
    int64_t (*action_cb)(void* ctx, int op, int a1, int a2, int a3);
    void (*local_summary_cb)(void* ctx, NzbgetRsFrontendSummary* out_summary);
    void (*local_messages_cb)(
        void* ctx, int tail_mode, uint32_t arg, int* out_skipped, void* emit_ctx,
        void (*emit_cb)(void* emit_ctx, uint32_t id, int kind, int64_t time, const char* text));
    void (*local_queue_cb)(
        void* ctx, void* emit_ctx,
        void (*emit_nzb_cb)(
            void* emit_ctx, int id, int priority, int active_dl,
            int64_t size, int64_t rem_size, int64_t paused_size,
            int paused_files, int rem_pars, const char* name),
        void (*emit_file_cb)(
            void* emit_ctx, size_t nzb_idx, int id, int active_dl,
            int paused, int64_t size, int64_t rem_size, const char* filename));
} NzbgetRsFrontendEnv;

int nzbget_rs_frontend_op(int op, const NzbgetRsFrontendEnv* env);

#include "nzbget_tls_cipher.h"

#ifdef __cplusplus
}
#endif

#endif
