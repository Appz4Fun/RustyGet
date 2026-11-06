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

// Stream-repair floor: arrays remain C++ owned and borrowed only during the call.
// Nonempty arrays are readable/aligned for count entries; empty arrays may be NULL.
// Probe indexes must identify donor entries. Invalid indexes abort before an
// out-of-bounds read. Overflowing reference C++ arithmetic is outside parity.
typedef struct { int64_t offset; int64_t size; } NzbgetRsStreamCompareRange;
int64_t nzbget_rs_stream_compare_floor(int64_t decoded_size,
    const NzbgetRsStreamCompareRange* holes, size_t hole_count,
    const NzbgetRsStreamCompareRange* donors, size_t donor_count,
    const int32_t* probes, size_t probe_count);

// raw may be NULL (treated as empty); otherwise it must point to a readable
// NUL-terminated string for the duration of the call. Results own their storage.
// Allocation failure or a Rust panic aborts; unwinding never crosses this ABI.
NzbgetRsBuf nzbget_rs_json_encode(const char* raw);
NzbgetRsBuf nzbget_rs_xml_encode(const char* raw);
// Caller-owned output; input_len is the known first-NUL position, not measured
// again. raw is readable through that NUL (NULL only when input_len==0).
// out is NULL (measure; cap ignored) or writable for cap bytes. SIZE_MAX rejects
// unrepresentable lengths/endpoints or overlap with input including its NUL,
// before any borrow/write. Never retry SIZE_MAX. Otherwise n excludes NUL:
// n<cap initializes result+NUL; insufficient output is unusable, retry with n+1.
// Output may be uninitialized; Rust never allocates/frees it. Latin1 requires
// at most 2*input_len bytes, plus one for NUL.
size_t nzbget_rs_latin1_to_utf8_n(const char* raw, size_t input_len, char* out, size_t cap);
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
NzbgetRsBuf nzbget_rs_twin_par2_sigs(const unsigned char* data, size_t len);
// The fingerprint of count files given as md5 texts (NULL entries empty) and
// lengths; their names are not part of it.
NzbgetRsBuf nzbget_rs_twin_fingerprint(const char* const* md5s, const unsigned long long* lengths, size_t count);
unsigned long long nzbget_rs_par2_block_size(const unsigned char* data, size_t len);
int nzbget_rs_par2_volume_blocks(const char* filename);
NzbgetRsBuf nzbget_rs_twin_kind(int primary, const char* primaryPrint, int dupe, const char* dupePrint);
NzbgetRsBuf nzbget_rs_twin_match(int own, const char* ownPrint, const char* target, const char* targetAlt,
	int donor, const char* donorPrint);
// "F\tsize\tpar2\tfilename\tsubject" per <file>, then "G\tgroup" and "S\tbytes\tmessage-id"
NzbgetRsBuf nzbget_rs_nzb_entries(const char* path);
// Release each result exactly once, with all fields unchanged. A zero buffer
// is also accepted. Copy the bytes before freeing if they must outlive the result.
void nzbget_rs_free(NzbgetRsBuf buf);

typedef struct
{
	int matched;
	size_t count;
} NzbgetRsWildResult;

// NULL pattern/text mean empty strings; otherwise they must be NUL-terminated.
// table: glibc's tolower table of the calling thread (*__ctype_tolower_loc(),
// valid for indexes -128..255; char_signed: CHAR_MIN < 0 of the caller), or NULL to use fold(byte 0..255), which returns
// tolower of that byte as the caller's char in the current locale.
// fold may be NULL when table is supplied. If both are NULL, returns {0, 0}
// without writing positions.
// positions is caller-owned writable storage for capacity pairs, disjoint from
// the inputs. NULL disables positions. Both result fields are valid on failure.
// count is the TOTAL capture count, even if only capacity pairs could be written;
// retry with count pairs if needed. Panics abort; no unwinding crosses the ABI.
NzbgetRsWildResult nzbget_rs_wild_match(const char* pattern, const char* text,
	int (*positions)[2], size_t capacity, const int* table, int char_signed, int (*fold)(int));

// RPC request decoders (rust/src/decode.rs), as WebUtil's:
// All buffers remain caller-owned; these functions allocate no result buffers.
// Base64: NULL input returns 0. Otherwise input is readable for length bytes,
// or, when length <= 0, through the NUL (strlen is truncated to uint32 as in
// C++). Output has room for len / 4 * 3 bytes, and is input or disjoint from it.
// Returns bytes written, without a terminator.
unsigned int nzbget_rs_decode_base64(const char* input, int length, char* output);
// raw is NULL (no-op) or a writable NUL-terminated string, decoded in place.
void nzbget_rs_json_decode(char* raw);
// text is NULL or NUL-terminated; valueLength is NULL or a writable int.
// Returns a pointer into text, or NULL on failure (including either NULL
// argument). Leaves valueLength unchanged on failure.
const char* nzbget_rs_json_next_value(const char* text, int* valueLength);

// CRC-32 of A followed by B from CRC(A), CRC(B) and B's length
// (rust/src/crc.rs); a length of 0 returns crc1
unsigned int nzbget_rs_crc32_combine(unsigned int crc1, unsigned int crc2, unsigned int len2);

// WebUtil's text helpers (rust/src/text.rs): the in-place ones take NULL (no-op)
// or a caller-owned writable NUL-terminated string. Panics abort.
// lower receives an ASCII hex letter and returns the caller's tolower result.
// It must not unwind or access raw. NULL lower is a no-op.
void nzbget_rs_xml_decode(char* raw, int (*lower)(int));
void nzbget_rs_xml_strip_tags(char* raw);
// isAlpha receives a byte in 0..255 and classifies it using the caller's locale
// and char signedness. It must not unwind or access raw. NULL isAlpha is a no-op.
void nzbget_rs_xml_remove_entities(char* raw, int (*isAlpha)(int));
void nzbget_rs_http_unquote(char* raw);
void nzbget_rs_url_decode(char* raw);
// NULL input means empty. Results are Rust-owned NUL-terminated buffers:
// copy before freeing with nzbget_rs_free, never with the C allocator.
NzbgetRsBuf nzbget_rs_url_encode(const char* raw);
NzbgetRsBuf nzbget_rs_latin1_to_utf8(const char* raw);

// WebUtil's finders (rust/src/webutil.rs): a pointer into the text and the
// value length, or NULL with valueLength untouched
const char* nzbget_rs_xml_find_tag(const char* xml, const char* tag, int* valueLength);
const char* nzbget_rs_json_find_field(const char* text, const char* field, int* valueLength);
// WebUtil::ParseContentDispositionFilename: data is NULL for no file name.
// Case folding as strncasecmp: glibc's tolower table (*__ctype_tolower_loc(),
// entries -128..255) indexed by unsigned byte, or fold(byte 0..255) when NULL.
NzbgetRsBuf nzbget_rs_content_disposition_filename(const char* contentDisposition,
	const int* table, int (*fold)(int));

// Util's string helpers (rust/src/util.rs). Format results are Rust-owned;
// copy and release with nzbget_rs_free. Numeric formatting uses LC_NUMERIC.
// Panics abort and never unwind through C++.
NzbgetRsBuf nzbget_rs_format_size(long long size);
NzbgetRsBuf nzbget_rs_format_speed(long long bytesPerSecond);
// NULL is treated as an empty string.
int nzbget_rs_alpha_num(const char* str);
// buffer is NULL (hashed as empty) or readable for (unsigned int)bufSize bytes.
unsigned int nzbget_rs_hash_bj96(const char* buffer, int bufSize, unsigned int initValue);
// Caller-owned writable NUL-terminated str; from/to are NUL-terminated and
// may point into str. Any NULL argument is a no-op. Empty patterns and growing
// replacements are no-ops; equal-length replacements retain C++ truncation.
void nzbget_rs_reduce_str(char* str, const char* from, const char* to);
// case folding: glibc's tolower table (*__ctype_tolower_loc()) or NULL for
// the callbacks: caseFold(byte 0..255) as strcasecmp, maskFold as WildMask's.
// Callbacks must not unwind or mutate input storage. They may both be NULL
// with a table; without a table either NULL callback returns false.
// NULL strings are empty; non-NULL strings must be readable and NUL-terminated.
int nzbget_rs_match_file_ext(const char* filename, const char* extensionList, const char* listSeparator,
	const int* table, int charSigned, int (*caseFold)(int), int (*maskFold)(int));

// URL::ParseUrl (rust/src/url.rs): parts (protocol, user, password, host,
// resource) as {start, length} in the address, start -1 when not set; a valid
// URL without a resource has resource start -1 (meaning "/")
typedef struct
{
	int valid;
	int port;
	ptrdiff_t parts[5][2];
} NzbgetRsUrlParts;
void nzbget_rs_parse_url(const char* address, NzbgetRsUrlParts* out);

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
	size_t (*regexNew)(void* user, const char* pattern, int bufSize);
	int (*regexMatch)(void* user, size_t regex, const char* text, int (*groups)[2], int capacity);
	void (*apply)(void* user, const NzbgetRsFeedOptions* options);
	void (*setMatch)(void* user, int status, int rule);
	const int* lowerTable; // *__ctype_tolower_loc() or NULL for fold
	int charSigned;
	int (*fold)(int);
} NzbgetRsFeedItem;
typedef struct NzbgetRsFeedFilter NzbgetRsFeedFilter;
// Copies filter (NULL means empty). The opaque handle is owned by Rust and
// must only be released with feed_filter_free; free(NULL) is harmless.
NzbgetRsFeedFilter* nzbget_rs_feed_filter_new(const char* filter);
void nzbget_rs_feed_filter_free(NzbgetRsFeedFilter* filter);
// NULL filter/item or missing callbacks are a no-op. All callbacks are
// required except fold when lowerTable is supplied. lowerTable points to
// entry zero of a valid int table spanning indices -128..255.
// Matching requires exclusive access to the filter and its regex handles.
void nzbget_rs_feed_filter_match(NzbgetRsFeedFilter* filter, const NzbgetRsFeedItem* item);

// Deobfuscation (rust/src/deobfuscation.rs): text as pointer and length.
// NULL means empty regardless of len; otherwise len bytes must be readable.
// Embedded NULs are preserved. Free the owned result with nzbget_rs_free.
int nzbget_rs_is_excessively_obfuscated(const char* str, size_t len);
NzbgetRsBuf nzbget_rs_deobfuscate(const char* str, size_t len);

// FileTypes (rust/src/filetypes.rs): name checks by number, in FileTypes.h's
// order (IsSevenZipExt 0 ... IsKnownCollectionExt 25); a sniffed extension is static
// ("" for none), its length in outLen. Inputs are borrowed byte spans; NULL
// means empty regardless of len. outLen may be NULL. Returned strings have
// static lifetime and must not be freed. Panics abort at the ABI boundary.
int nzbget_rs_file_type(int which, const char* str, size_t len);
const char* nzbget_rs_sniff_extension(const unsigned char* header, size_t len, size_t* outLen);
const char* nzbget_rs_sniff_extension_file(const char* path, size_t pathLen, size_t* outLen);

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
int nzbget_rs_reserved_char(char c);
// path is NULL (no-op) or a writable NUL-terminated string, owned by the caller.
void nzbget_rs_normalize_path_separators(char* path);

// WebProcessor's request decisions (rust/src/webserver.rs). Header kinds:
// 0 other, 1 Content-Length (number), 2 credentials (value), 3 credentials
// too long, 4 Accept-Encoding (number: gzip), 5 Origin, 6 Auth-Token cookie,
// 7 X-Forwarded-For, 8 If-None-Match, 9 keep-alive, 10 end of headers
// line is NULL (empty) or len initialized bytes. Tests stop at its first NUL;
// a locale-induced early match still reads the value at its fixed offset,
// bounded by len. Outputs may be NULL and must not alias the input.
int nzbget_rs_web_header(const char* line, size_t len, int authInfoEmpty,
	size_t* valueStart, size_t* valueLen, int* number);
// NULL URL means empty; embedded NUL ends the URL. Outputs may be NULL.
// Free both returned buffers with nzbget_rs_free (including on exceptions).
NzbgetRsBuf nzbget_rs_web_parse_url(const char* url, size_t len, int* redirect, NzbgetRsBuf* auth);
typedef struct
{
	const char* users[6]; // control, restricted, add: username, password
	const char* authorizedIp;
	const char* remoteAddr;
	const char* authInfo;
	const char* authToken;
	const char* serverTokens[3];
	const int* lowerTable; // as for WildMask
	int charSigned;
	int (*fold)(int);
} NzbgetRsWebCredentials;
typedef struct
{
	int authorized;
	int access; // EUserAccess, -1 to leave it
	ptrdiff_t authCut; // where m_authInfo is cut, -1 for none
	int warn;
} NzbgetRsWebCheck;
// NULL input denies access; NULL out is a no-op. Output must not alias inputs.
void nzbget_rs_web_check_credentials(const NzbgetRsWebCredentials* input, NzbgetRsWebCheck* out);
int nzbget_rs_web_authorized_ip(const char* option, const char* remote, const int* table, int charSigned, int (*fold)(int));

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

// XmlCommand's request parameters (rust/src/rpcparams.rs), parsed in place in
// the writable NUL-terminated request. skip_to_params: PrepareParams for a JSON
// POST (the position after "params", or the request emptied). next_param: what
// 0 int, 1 bool (0/1 in *intValue), 2 string (*strValue points into the
// request); get: a GET query string, else json: JSON-RPC, else XML-RPC. Moves
// *request past the parameter; returns 1 with a value, else 0.
// NULL request, *request or the required value output, and unknown what, are
// rejected without mutation. Outputs and pointer slots must be disjoint from
// each other and the request buffer. Storage stays caller-owned; string results
// borrow the request and must not be freed. Panics cannot unwind across the ABI.
char* nzbget_rs_rpc_skip_to_params(char* request);
int nzbget_rs_rpc_next_param(char** request, int get, int json, int what, int* intValue, char** strValue);

// XmlRpcProcessor's routing (rust/src/rpcroute.rs). protocol: the
// ERpcProtocol of an RPC URL (rpUndefined if none). route: Dispatch's method
// name (into a 100-byte buffer), where the parameters start (*params, into url
// for GET, else request) and the JSON-RPC id (*id, *idLen; null if none or its
// legacy C-int length exceeds 4096). Inputs may be NULL (empty), otherwise
// they are NUL-terminated. route's output pointers must be non-NULL, writable,
// and disjoint; methodName has at least 100 bytes. params/id borrow the
// caller's input storage; do not free them or retain them beyond its lifetime.
// envelope: BuildResponse's text before (head) and after (tail) the response;
// head and tail must be non-NULL, aligned, writable and disjoint.
// each result owns its buffer, independently of the inputs. Free both exactly
// once with nzbget_rs_free. Panics abort; unwinding never crosses the ABI.
// access: EUserAccess (0 control, 1 restricted, 2 add); JSON-P changes control
// to restricted and leaves other access unchanged. protocol is ERpcProtocol
// (0 undefined, 1 XML-RPC, 2 JSON-RPC, 3 JSON-P).
// decide: 0 runs the command, 1 answers with an error inside the JSON-P
// callback, 2 answers with a plain error; *code and *text get that error (0
// and NULL when it runs). httpMethod is EHttpMethod (0 POST, 1 GET); safe,
// error and jsonp are true when nonzero. NULL outputs discard that result.
// Non-NULL outputs must be aligned, writable and disjoint. The returned text
// is a static NUL-terminated string; do not modify or free it.
int nzbget_rs_rpc_protocol(const char* url);
void nzbget_rs_rpc_route(const char* url, const char* request, int get, int protocol, char* methodName, const char** params, const char** id, int* idLen);
void nzbget_rs_rpc_envelope(int protocol, int fault, const char* callback, const char* id, NzbgetRsBuf* head, NzbgetRsBuf* tail);
int nzbget_rs_rpc_access(int protocol, int access);
int nzbget_rs_rpc_decide(int protocol, int httpMethod, int safe, int error, int jsonp, int* code, const char** text);

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
char* nzbget_rs_trim_line(char* s, int rightOnly);
size_t nzbget_rs_trim_string(char* data, size_t len, int left, int right, int sanitize, size_t* start, int (*rightSpace)(int));
int nzbget_rs_ends_with(const char* s, size_t len, const char* suffix, size_t suffixLen, int caseSensitive);
NzbgetRsBuf nzbget_rs_format_buffer(const char* buf, int len);
long long nzbget_rs_parse_rfc822_date_time(const char* s);

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
int nzbget_rs_parse_time(const char* time, int* hours, int* minutes);
int nzbget_rs_parse_week_days(const char* weekDays, int* bits);
int nzbget_rs_validate_option_name(const char* name, int (*predefined)(void* ctx, const char* name), void* ctx);
int nzbget_rs_option_restricted(const char* name, int signedChar);
void nzbget_rs_convert_old_option(const char* option, const char* value, NzbgetRsBuf* newOption, NzbgetRsBuf* newValue);
int nzbget_rs_has_script(const char* list, const char* name);
int nzbget_rs_parse_category_source(const char* value);

// ExtensionLoader::V1 (rust/src/extload.rs): a pre-manifest script's header.
// parse returns a handle (free with ext_v1_free) or NULL when the file isn't
// an extension script. Strings are borrowed from the handle (not NUL-
// terminated). text: 0 about (untrimmed), 1 queue events, 2 task time,
// 3 description line i, 4 requirement i. count: 0 description lines,
// 1 requirements, 2 options, 3 commands. item_text (command 0 option, 1
// command): 0 section name, 1 section prefix, 2 name, 3 action, 4 description
// line j. item_count: 0 section multi, 1 description lines, 2 select values.
// select: option i's value (j == SIZE_MAX) or select value j: 1 a number in
// *num, 0 a text in *text, -1 none.
typedef struct NzbgetRsStr
{
	const char* data;
	size_t len;
} NzbgetRsStr;
typedef struct NzbgetRsExtV1 NzbgetRsExtV1;
// data is borrowed for this call; NULL is empty. The result owns its bytes.
// rightSpace classifies byte 0..255 as Util::TrimRight does in the caller's
// current C locale and char signedness. It must not throw. NULL returns NULL.
// All returned strings are length-delimited, may contain NUL, are immutable,
// and remain valid until free(h); never free them separately. Free accepts NULL.
// Invalid indexes/fields and NULL handles return empty/zero (select: -1).
// select outputs may be NULL: if the required output is NULL, returns -1;
// otherwise writes only that output. Outputs must not overlap the handle.
// Rust panics abort; no exception may unwind through these functions.
NzbgetRsExtV1* nzbget_rs_ext_v1_parse(const char* data, size_t len, int (*rightSpace)(int));
// next returns one std::getline line (without '\n'): 1 line, 0 EOF, -1 error.
// Its bytes remain readable until next is called again. On error parsing
// returns NULL. next must catch all C++ exceptions; rethrow after read returns.
// NULL callbacks return NULL. No callback is retained after this call.
typedef int (*NzbgetRsExtV1Next)(void* context, NzbgetRsStr* line);
NzbgetRsExtV1* nzbget_rs_ext_v1_read(NzbgetRsExtV1Next next, void* context, int (*rightSpace)(int));
void nzbget_rs_ext_v1_free(NzbgetRsExtV1* h);
int nzbget_rs_ext_v1_kind(const NzbgetRsExtV1* h);
NzbgetRsStr nzbget_rs_ext_v1_text(const NzbgetRsExtV1* h, int which, size_t i);
size_t nzbget_rs_ext_v1_count(const NzbgetRsExtV1* h, int which);
NzbgetRsStr nzbget_rs_ext_v1_item_text(const NzbgetRsExtV1* h, int command, size_t i, int field, size_t j);
size_t nzbget_rs_ext_v1_item_count(const NzbgetRsExtV1* h, int command, size_t i, int which);
int nzbget_rs_ext_v1_select(const NzbgetRsExtV1* h, size_t i, size_t j, double* num, NzbgetRsStr* text);

// WebDownloader's HTTP decisions (rust/src/webdownload.rs). check_response:
// result 0 running, 1 running a redirect, 2 connect error, 3 not found,
// 4 failed; setStatus/httpStatus the status code read; warn 0 none,
// 1 connection closed, 2 failed with the whole line, 3 failed with the text at
// statusOffset. http_header: 0 nothing, 1 Content-Length (*value), 2 gzip,
// 3 Content-Disposition, 4 Location (at line + *value). http_redirect: the
// address a Location leads to (free with nzbget_rs_free).
typedef struct NzbgetRsHttpResponse
{
	int result;
	int setStatus;
	int httpStatus;
	int warn;
	size_t statusOffset;
} NzbgetRsHttpResponse;
void nzbget_rs_http_check_response(const char* response, NzbgetRsHttpResponse* out);
int nzbget_rs_http_header(const char* line, int redirecting, int* value);
NzbgetRsBuf nzbget_rs_http_redirect(const char* oldUrl, const char* location);

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

// CommandLineParser (rust/src/cmdline.rs): its constructor, over the argv array
// getopt permutes. The fields are set through the sink (field numbers in
// rust/src/cmdline.rs: field, edit); steal moves argv[index] into a field
// (std::move, the entry becomes null). Callbacks return 0, or -1 when they
// threw (parsing stops, the call returns -1: rethrow then). No callback may
// unwind. String arguments are borrowed for the callback; copy them if retained.
// steal keeps a moved allocation alive until its destination is overwritten
// or parsing finishes. The argv array
// initially contains argc non-NULL strings followed by a NULL sentinel.
// NULL inputs/entries or missing
// callbacks return -1. getopt globals must not be used concurrently.
typedef struct NzbgetRsCmdlineSink
{
	// The platform's getopt_long with the C++ option table; optional only
	// when useLong == 0. Returns libc's option code (or -1 at end).
	int (*getoptLong)(int argc, char** argv);
	void* ctx;
	int (*setInt)(void* ctx, int field, int value);
	int (*setStr)(void* ctx, int field, const char* value);
	int (*steal)(void* ctx, int field, int index);
	int (*pushOption)(void* ctx, const char* value);
	int (*pushId)(void* ctx, int id);
	int (*pushName)(void* ctx, const char* value);
	int (*error)(void* ctx, const char* msg);
} NzbgetRsCmdlineSink;
int nzbget_rs_cmdline_parse(int argc, char** argv, int useLong, const NzbgetRsCmdlineSink* sink);

// FileSystem's file operations (rust/src/fsops.rs, POSIX). error_message:
// writes at most 1022 bytes plus NUL (the original BString capacity argument)
// to a 1024-byte buffer; NULL is a no-op. Buffers returned by these functions
// are Rust-owned and must be freed with nzbget_rs_free, never free/delete.
// Panics abort; they do not unwind across the ABI.
// stat: borrowed for the call, takes a borrowed NUL-terminated path and returns
// -errno on failure, 1 for a regular file, 2 for a directory, 0 otherwise.
// It follows symlinks, uses the caller's stat ABI, and must not throw/unwind.
typedef int (*NzbgetRsFsStat)(const char* path);
void nzbget_rs_error_message(int errnum, char* buf);
// NULL paths or callbacks return 0.
int nzbget_rs_file_exists(const char* path, NzbgetRsFsStat stat);
int nzbget_rs_directory_exists(const char* path, NzbgetRsFsStat stat);
// 1 on success, 0 on failure; every non-NULL errmsg is initialized (empty on
// success or invalid NULL arguments). errmsg must not overlap the input.
int nzbget_rs_force_directories(const char* path, NzbgetRsBuf* errmsg, NzbgetRsFsStat stat);
// NULL dir/basename mean empty strings; a NULL stat reports no collisions.
NzbgetRsBuf nzbget_rs_make_unique_filename(const char* dir, const char* basename, NzbgetRsFsStat stat);
// NULL filename means empty; NULL home leaves the filename unchanged.
NzbgetRsBuf nzbget_rs_expand_home_path(const char* filename, const char* home);

// ParParser's name checks (rust/src/parparser.rs).
// Names are borrowed and read only through the first NUL or 1023 bytes.
// fold takes a byte (0..255), applies the caller's tolower(char), and must not
// unwind. NULL names/fold return 0. Outputs are optional, writable C ints;
// they may alias each other (blocks is written last), and change only on success.
// No ownership transfer or allocation crosses the ABI; panics abort.
// table: glibc's tolower table (*__ctype_tolower_loc()) read for a char of
// the given signedness instead of calling fold per byte; NULL calls fold.
int nzbget_rs_parse_par_filename(const char* name, int confirmed, int* baseLen, int* blocks,
	const int* table, int charSigned, int (*fold)(int));
int nzbget_rs_same_par_collection(const char* name1, const char* name2, int confirmed,
	const int* table, int charSigned, int (*fold)(int));
int nzbget_rs_find_main_pars(const char* path, int collect, const int* table, int charSigned,
	int (*fold)(int), void (*addFile)(void* ctx, const char* name, size_t len), void* ctx);

// RarVolume's header parsing (rust/src/rarreader.rs). rar_read parses the
// content source into *state (read and written back) and calls addFile for each
// file read, in order, also when the parse fails; 1 when the volume was read
// to its end. size/read are the ContentSource; rar3Prepare, rar5Prepare, init
// (iv NULL: rar3's derived IV), decrypt (one 16-byte block) and free are the
// header cipher. addFile commits a completed header before the trailing seek;
// setDataOffset updates the last file to position minus its packed size after
// that seek succeeds. File/name pointers are borrowed only for addFile: copy
// their contents there; never retain or free them. read/decrypt buffers are
// borrowed writable storage of len/16 bytes. Callbacks must not throw; int
// callbacks return nonzero for success; a rejected addFile stops parsing. NULL io/state
// fails without callbacks; NULL callbacks are allowed (missing addFile drops
// files and suppresses setDataOffset). ctx is opaque and may be NULL if the
// callbacks support it. io must remain immutable and state exclusively borrowed
// until rar_read returns, including during callbacks.
typedef struct NzbgetRsRarFile
{
	const char* filename;
	size_t filenameLen;
	uint32_t time;
	uint32_t attr;
	int64_t size;
	int splitBefore;
	int splitAfter;
	uint32_t method;
	int stored;
	int64_t packedSize;
	int64_t dataOffset;
	int encryptedData;
	int hasSalt;
	uint8_t salt[8];
	int hasCrypt;
	uint64_t cryptVersion;
	uint64_t cryptFlags;
	uint8_t cryptKdfCount;
	uint8_t cryptSalt[16];
	uint8_t cryptIv[16];
	uint8_t cryptCheckValue[12];
	int cryptHasCheck;
} NzbgetRsRarFile;
typedef struct NzbgetRsRarIo
{
	void* ctx;
	int64_t (*size)(void* ctx);
	int (*read)(void* ctx, int64_t offset, uint8_t* buffer, int64_t len);
	int (*rar3Prepare)(void* ctx, const uint8_t* salt);
	int (*rar5Prepare)(void* ctx, uint8_t kdfCount, const uint8_t* salt);
	int (*init)(void* ctx, int keyBits, const uint8_t* iv);
	int (*decrypt)(void* ctx, const uint8_t* in, uint8_t* out);
	void (*free)(void* ctx);
	int (*addFile)(void* ctx, const NzbgetRsRarFile* file);
	void (*setDataOffset)(void* ctx, int64_t position);
} NzbgetRsRarIo;
typedef struct NzbgetRsRarVolume
{
	int version;
	uint32_t volumeNo;
	int newNaming;
	int hasNextVolume;
	int multiVolume;
	int encrypted;
	int hasPassword;
	uint8_t decryptBuf[16];
	uint8_t decryptPos;
} NzbgetRsRarVolume;
int nzbget_rs_rar_read(const NzbgetRsRarIo* io, NzbgetRsRarVolume* state);

// RarRenamer::RenameFiles (rust/src/rarrenamer.rs): the volumes read from dir
// gathered into sets; the sets not properly named are renamed through rename
// (index into volumes, the new path). fileExists is FileSystem::FileExists;
// matchPart the RegEx ".*.part([0-9]+)\\.rar$" (group 1's start and length).
// All input buffers are borrowed for the call. Callback strings are borrowed
// only until the callback returns; callbacks must copy them to retain them and
// must not free them or throw. No allocation crosses the ownership boundary.
// NULL dir/env or NULL volumes with nonzero count makes the call a no-op.
// NULL names and file arrays are treated as empty. Rust panics abort, never
// unwind through C++.
typedef struct NzbgetRsRarRenFile
{
	const char* name;
	size_t nameLen;
	int64_t size;
	int splitBefore;
	int splitAfter;
} NzbgetRsRarRenFile;
typedef struct NzbgetRsRarRenVolume
{
	const char* filename;
	size_t filenameLen;
	int version;
	uint32_t volumeNo;
	int multiVolume;
	int hasNextVolume;
	int newNaming;
	const NzbgetRsRarRenFile* files;
	size_t fileCount;
} NzbgetRsRarRenVolume;
typedef struct NzbgetRsRarRenEnv
{
	void* ctx;
	int (*fileExists)(void* ctx, const char* path);
	int (*matchPart)(void* ctx, const char* name, size_t* start, size_t* len);
	void (*rename)(void* ctx, size_t index, const char* dest);
} NzbgetRsRarRenEnv;
void nzbget_rs_rar_rename_files(const char* dir, const NzbgetRsRarRenVolume* volumes, size_t count, const NzbgetRsRarRenEnv* env);

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
typedef int (*NzbgetRsFormatParName)(void* ctx, int vol, const char** data, size_t* len);
NzbgetRsBuf nzbget_rs_direct_par_name(const char* oldName, const char* destDir, const char* setId,
	char separator, int* vol, int (*exists)(void* ctx, const char* path), NzbgetRsFormatParName format, void* ctx);
typedef struct NzbgetRsParName
{
	const char* filename;
	const char* setId;
} NzbgetRsParName;
int nzbget_rs_direct_need_rename_pars(const NzbgetRsParName* pars, size_t count, int (*fold)(int));

// ManifestFile::Load (rust/src/manifest.rs, rust/src/json.rs: Boost.JSON's
// parse with the default options) on the manifest text the C++ read: 1 when
// loaded, after loaded got the manifest; 0 refused (a required key missing
// or of another kind); -1 a syntax error; -2 not a JSON object; -3 an
// option, command or section that isn't an object. On 0 and -3, loaded gets
// the partial writes preceding failure. Texts/options/commands are appended; sections apply
// to both existing and appended entries. The manifest and every
// string in it are borrowed for the loaded call only; strings are not
// NUL-terminated. fields are author, main, homepage, about, version, name,
// displayName, kind, license, taskTime, queueEvents, nzbgetMinVersion; has[i]
// says whether fields[i] was assigned before validation stopped. NULL text is empty; NULL loaded
// is not called. loaded must not throw; panics abort.
typedef struct NzbgetRsManifestSection
{
	int multi;
	NzbgetRsStr name;
	NzbgetRsStr prefix;
} NzbgetRsManifestSection;
typedef struct NzbgetRsManifestSelect
{
	int isString;
	double number;
	NzbgetRsStr string;
} NzbgetRsManifestSelect;
typedef struct NzbgetRsManifestOption
{
	NzbgetRsManifestSection section;
	NzbgetRsStr name;
	NzbgetRsStr displayName;
	const NzbgetRsStr* description;
	size_t descriptionCount;
	NzbgetRsManifestSelect value;
	const NzbgetRsManifestSelect* select;
	size_t selectCount;
} NzbgetRsManifestOption;
typedef struct NzbgetRsManifestCommand
{
	NzbgetRsManifestSection section;
	NzbgetRsStr name;
	NzbgetRsStr displayName;
	NzbgetRsStr action;
	const NzbgetRsStr* description;
	size_t descriptionCount;
} NzbgetRsManifestCommand;
typedef struct NzbgetRsManifest
{
	NzbgetRsStr fields[12];
	int has[12];
	const NzbgetRsStr* description;
	size_t descriptionCount;
	const NzbgetRsStr* requirements;
	size_t requirementsCount;
	const NzbgetRsManifestOption* options;
	size_t optionCount;
	const NzbgetRsManifestCommand* commands;
	size_t commandCount;
	const NzbgetRsManifestSection* sections;
	size_t sectionCount;
} NzbgetRsManifest;
// Limits from this build of Boost.JSON, in decoded bytes / input elements.
typedef struct NzbgetRsJsonLimits
{
	size_t stringSize;
	size_t arraySize;
	size_t objectSize;
} NzbgetRsJsonLimits;
int nzbget_rs_manifest_load(const char* text, size_t len, NzbgetRsJsonLimits limits, void* ctx,
	void (*loaded)(void* ctx, const NzbgetRsManifest* manifest));

// TestServerXmlCommand's JSON body (rust/src/rpcparams.rs with the parser
// above): 1 when parsed, after parsed got the parameters (strings borrowed
// for the call only, whole - NULs included - and not NUL-terminated); 0 not
// JSON ("Invalid JSON"); -1 "Invalid parameters". NULL request is empty;
// NULL parsed is not called. parsed must not throw; panics abort.
typedef struct NzbgetRsTestServerParams
{
	NzbgetRsStr host;
	NzbgetRsStr username;
	NzbgetRsStr password;
	NzbgetRsStr cipher;
	int port;
	int timeout;
	int certVerifLevel;
	int encryption;
} NzbgetRsTestServerParams;
int nzbget_rs_test_server_params(const char* request, NzbgetRsJsonLimits limits, void* ctx,
	void (*parsed)(void* ctx, const NzbgetRsTestServerParams* params));

// ParRenamer::Execute (rust/src/parrenamer.rs). The callbacks get borrowed
// NUL-terminated strings, valid for the call only, and return -1 (read: -1)
// for a C++ exception, which ends the run (execute then returns 0; the state
// holds what was done until then). stopped: 1 stopped. progress: the label
// and stage set, then UpdateProgress. message kinds: 0 info, 1 warning,
// 2 error. listDir / findMainPars call add for each name (DirBrowser's
// snapshot / ParParser::FindMainPars). dirExists / fileExists /
// ignoredExt (Util::MatchFileExt with ParIgnoreExt): 1 yes. fileSize writes
// FileSystem::FileSize. read: the bytes DiskFile read from the start of the
// file into buf (at most len), -2 when it could not be opened, -3 when the
// read failed. loadPar: 1 loaded (writes the source map size), 0 when the
// packets could not be loaded or the file is empty. parEntry translates the
// next source map entry and calls add once (damaged or local name/hash).
// closePar releases the loaded repairer; the caller must also release it on
// an aborted run. stateChanged publishes members before callbacks, cannot
// fail or unwind, and must not retain the borrowed state pointer. It writes
// back detectMissing, which callbacks can change through the C++ setter.
// previousDirs / rememberDir retain the directory list across runs.
// destination calls add once with the current destination after initial progress;
// a NULL callback uses execute's destDir argument instead. All input strings
// are copied before callbacks may replace the caller's storage. All sink
// and add pointers are borrowed, non-NULL, and valid only during the callback.
// No buffers or objects transfer ownership across the ABI. rename: RenameFile
// without the count, 1 when moved. fold is ParParser::FilenameFold. NULL
// callbacks mean: not stopped, nothing printed, empty lists, no files, reads
// that can't open, nothing loaded, no renames, identity fold.
typedef struct NzbgetRsParRenamerState
{
	int fileCount;
	int curFile;
	int renamedCount;
	int hasMissedFiles;
	int hasDamagedParFiles;
	int detectMissing;
} NzbgetRsParRenamerState;
typedef struct NzbgetRsParRenamerEnv
{
	void* ctx;
	int (*stopped)(void* ctx);
	int (*progress)(void* ctx, const char* label, int stage);
	int (*message)(void* ctx, int kind, const char* text);
	int (*debug)(void* ctx, const char* text);
	int (*listDir)(void* ctx, const char* dir, void* sink, void (*add)(void* sink, const char* name));
	int (*dirExists)(void* ctx, const char* path);
	int (*fileExists)(void* ctx, const char* path);
	int (*fileSize)(void* ctx, const char* path, int64_t* size);
	int64_t (*read)(void* ctx, const char* path, uint8_t* buf, size_t len);
	int (*loadPar)(void* ctx, const char* path, size_t* count);
	int (*parEntry)(void* ctx, void* sink,
		void (*add)(void* sink, int damaged, const char* name, const char* hash));
	void (*closePar)(void* ctx);
	int (*findMainPars)(void* ctx, const char* dir, void* sink, void (*add)(void* sink, const char* name));
	int (*registerParred)(void* ctx, const char* name);
	int (*rename)(void* ctx, const char* src, const char* dst);
	int (*ignoredExt)(void* ctx, const char* name);
	int (*fold)(int byte);
	void (*stateChanged)(void* ctx, NzbgetRsParRenamerState* state);
	int (*previousDirs)(void* ctx, const char* unused, void* sink, void (*add)(void* sink, const char* name));
	int (*rememberDir)(void* ctx, const char* dir);
	int (*destination)(void* ctx, const char* unused, void* sink, void (*add)(void* sink, const char* dir));
} NzbgetRsParRenamerEnv;
int nzbget_rs_par_renamer_execute(const NzbgetRsParRenamerEnv* env, NzbgetRsParRenamerState* state,
	const char* infoName, const char* destDir, char separator);

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
typedef struct NzbgetRsFsOps
{
	void* ctx;
	int (*pathType)(void* ctx, const char* path);
	int (*removeDir)(void* ctx, const char* path);
	int (*deleteFile)(void* ctx, const char* path);
	int (*deleteLink)(void* ctx, const char* path, int type);
	int (*list)(void* ctx, const char* dir, void* sink, void (*add)(void* sink, const char* name));
	int (*fileExists)(void* ctx, const char* path);
	int (*dirExists)(void* ctx, const char* path);
	int (*moveFile)(void* ctx, const char* from, const char* to);
	int (*lastError)(void* ctx, void* sink, void (*set)(void* sink, const char* text));
	// sets errno to ENAMETOOLONG: delete roots (without trailing separators or
	// "." components) and paths below them longer than 1023 bytes are refused
	int (*tooLong)(void* ctx);
} NzbgetRsFsOps;
int nzbget_rs_delete_directory(const NzbgetRsFsOps* ops, const char* dir, char separator);
int nzbget_rs_delete_directory_with_content(const NzbgetRsFsOps* ops, const char* dir, char separator, NzbgetRsBuf* errmsg);
int nzbget_rs_rename_bak(const NzbgetRsFsOps* ops, const char* filename, const char* bakPart, int removeOldExtension,
	NzbgetRsBuf* newName);
// FileSystem::GetFileExtension: the offset of the last '.', or -1.
ptrdiff_t nzbget_rs_file_extension(const char* name, size_t len);

// The binary remote protocol's frame (rust/src/binrpc.rs). base is the
// request base as received (SNzbRequestBase, 76 bytes, network byte order).
// check: 1 when the credentials match (strcmp as BinRpcProcessor compared
// them; an empty control user isn't compared), *type the request type.
// name: a static string for the log. size_ok: 1, or 0 with *expected and
// *received set for a known type whose struct size differs from sizes[type].
// bool_response: the bytes SendBoolResponse sends (free with nzbget_rs_free).
// trailing_ok: 1 when 0 <= len <= max. NULL outputs aren't written; a NULL
// or short base fails, as does a missing sizes entry for a known type,
// leaving outputs untouched. NULL control strings/text mean empty strings.
// Non-NULL inputs must be readable for their lengths (strings through NUL);
// non-NULL uint32_t pointers must be aligned and outputs writable. Outputs
// may overlap inputs; inputs are read first. Panics abort.
int nzbget_rs_binrpc_check(const unsigned char* base, size_t len, const char* controlUser, const char* controlPassword,
	uint32_t* type);
const char* nzbget_rs_binrpc_name(uint32_t type);
int nzbget_rs_binrpc_size_ok(const unsigned char* base, size_t len, const uint32_t* sizes, size_t count,
	uint32_t* expected, uint32_t* received);
NzbgetRsBuf nzbget_rs_binrpc_bool_response(int success, const char* text);
int nzbget_rs_binrpc_trailing_ok(int64_t len, int64_t max);
// Default single-entry decoder retained for BuildFileList; outputs may be uninitialized.
int nzbget_rs_binrpc_read_entry(const char* data, size_t len, size_t* at, uint32_t* ints, size_t nints,
	const size_t* fields, size_t* starts, size_t nfields);
// The entries of a list response (List, Log, PostQueue, History), one
// struct per entry, ints in host order, strings as bytes and a length
// (without the NUL; a NULL string needs length 0). The C++ collects a
// response's entries under its locks and one encode call makes the trailing
// data: each entry's ints big-endian, the lengths (with their NULs) in the
// fields of its strings, the strings with their NULs, zeros to 4 bytes
// counted in its last length, as the wire has them; ints an entry doesn't
// set are 0. The bytes are allocated once (free with nzbget_rs_free); no
// bytes, or more than INT_MAX, come back empty (NULL data, length 0).
// The strings are read during the call only. Read the same entries back
// with read_nzbs, read_params and read_files: up to max entries from *at
// of len bytes into out (which may be uninitialized), *at moved past them;
// fewer than max (the number read) when an entry or a string, up to its
// NUL, doesn't fit in len. The strings point into data and end at their
// NULs (their len is not set: 0). The listings are the client's printed
// lines of up to count entries (each with its newline, as a string; free
// with nzbget_rs_free): [INFO] text for a log message (nothing for an
// unknown kind), [id] name, stage, n% for a post-processing job, [id] name
// (...) for an NZB, URL or hidden item of the history (nothing for other
// kinds; unknown stages and statuses have no text); they end at an entry
// that doesn't fit in len.
typedef struct
{
	const char* data;
	size_t len;
} NzbgetRsBinText;
typedef struct
{
	uint32_t id, kind, sizeLo, sizeHi, remainingSizeLo, remainingSizeHi, pausedSizeLo, pausedSizeHi, pausedCount,
		remainingParCount, priority, match;
	NzbgetRsBinText filename, name, destDir, category, queuedFilename;
} NzbgetRsBinNzb;
typedef struct
{
	uint32_t nzbIndex; // counts from 1
	NzbgetRsBinText name, value;
} NzbgetRsBinParam;
typedef struct
{
	uint32_t id, nzbIndex, sizeLo, sizeHi, remainingSizeLo, remainingSizeHi, paused, filenameConfirmed, activeDownloads,
		match;
	NzbgetRsBinText subject, filename;
} NzbgetRsBinFile;
typedef struct
{
	uint32_t id, kind, time;
	NzbgetRsBinText text;
} NzbgetRsBinLog;
typedef struct
{
	uint32_t id, stage, stageProgress, fileProgress, startTime, stageTime;
	NzbgetRsBinText filename, name, destDir, progressLabel;
} NzbgetRsBinPost;
typedef struct
{
	uint32_t id, kind, time, sizeLo, sizeHi, fileCount, parStatus, scriptStatus, urlStatus;
	NzbgetRsBinText name;
} NzbgetRsBinHistory;
NzbgetRsBuf nzbget_rs_binrpc_encode_list(const NzbgetRsBinNzb* nzbs, size_t nzbCount, const NzbgetRsBinParam* params,
	size_t paramCount, const NzbgetRsBinFile* files, size_t fileCount);
NzbgetRsBuf nzbget_rs_binrpc_encode_log(const NzbgetRsBinLog* log, size_t count);
NzbgetRsBuf nzbget_rs_binrpc_encode_post_queue(const NzbgetRsBinPost* jobs, size_t count);
NzbgetRsBuf nzbget_rs_binrpc_encode_history(const NzbgetRsBinHistory* history, size_t count);
// Small responses can borrow caller scratch. ownedCapacity == 0 means
// borrowed (never pass it to nzbget_rs_free); otherwise free
// NzbgetRsBuf {data, len, ownedCapacity}. Null or insufficient scratch uses
// one owned allocation. capacity must include the final NUL. The scratch
// buffer must stay alive until the response is sent, and must not overlap
// any input array/string. Oversized responses still return empty data.
typedef struct
{
    char* data;
    size_t len, ownedCapacity;
} NzbgetRsBinResponse;
NzbgetRsBinResponse nzbget_rs_binrpc_encode_list_into(const NzbgetRsBinNzb* nzbs, size_t nzbCount,
    const NzbgetRsBinParam* params, size_t paramCount, const NzbgetRsBinFile* files, size_t fileCount,
    char* scratch, size_t capacity);
NzbgetRsBinResponse nzbget_rs_binrpc_encode_log_into(const NzbgetRsBinLog* log, size_t count, char* scratch, size_t capacity);
NzbgetRsBinResponse nzbget_rs_binrpc_encode_post_queue_into(const NzbgetRsBinPost* jobs, size_t count,
    char* scratch, size_t capacity);
NzbgetRsBinResponse nzbget_rs_binrpc_encode_history_into(const NzbgetRsBinHistory* history, size_t count,
    char* scratch, size_t capacity);
size_t nzbget_rs_binrpc_read_nzbs(const char* data, size_t len, size_t* at, NzbgetRsBinNzb* out, size_t max);
size_t nzbget_rs_binrpc_read_params(const char* data, size_t len, size_t* at, NzbgetRsBinParam* out, size_t max);
size_t nzbget_rs_binrpc_read_files(const char* data, size_t len, size_t* at, NzbgetRsBinFile* out, size_t max);
NzbgetRsBuf nzbget_rs_binrpc_log_listing(const char* data, size_t len, uint32_t count);
NzbgetRsBuf nzbget_rs_binrpc_post_queue_listing(const char* data, size_t len, uint32_t count);
NzbgetRsBuf nzbget_rs_binrpc_history_listing(const char* data, size_t len, uint32_t count);
// EditQueue's trailing data (text, ids, names). edit_sizes: 1 when the
// client's counts and lengths are non-negative, nrIds <= 10000000, nrNames <=
// namesLen and nrIds*4 + textLen + namesLen == dataLen. edit_split: 1 when
// the nrIds big-endian ids after the text and the nrNames names after them
// fit in len, each name ending within it, with the ids written into ids
// (host order); 0 (ids untouched) otherwise. Nothing is allocated. data is
// readable for len bytes; ids is writable for nrIds (NULL for 0).
// is_url: 1 when content starts with "http:/" or "https:/" in any case (NULL
// is empty).
int nzbget_rs_binrpc_edit_sizes(int32_t nrIds, int32_t nrNames, int32_t namesLen, int32_t textLen, uint32_t dataLen);
int nzbget_rs_binrpc_edit_split(const char* data, size_t len, size_t textLen, size_t nrIds, size_t nrNames,
	uint32_t* ids);
int nzbget_rs_binrpc_is_url(const char* content);
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
int nzbget_rs_nzb_declared_parts(const char* subject, const NzbgetRsCtype* ct);

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
void nzbget_rs_url_completed(int downloaderStatus, int deleting, int* urlStatus, int* deleteStatus, int* retry);

// ---- dupesearch (daemon/dupesearch/) ----
// DonorScore (rust/src/donorscore.rs): Base, Target (twin 0/1) and Rerank;
// rerank writes n scores parallel to entries into out (both NULL for n 0).
typedef struct NzbgetRsDonorEntry
{
	int score;
	double alive;
	int twin;
	int64_t bytes;
	int grabs;
} NzbgetRsDonorEntry;
int nzbget_rs_donorscore_base(int pickScore);
int nzbget_rs_donorscore_target(double alive, int twin);
void nzbget_rs_donorscore_rerank(const NzbgetRsDonorEntry* entries, size_t n, int64_t pickBytes, int* out);

// ReleaseName (rust/src/releasename.rs): Clean, Normalize, ShortQuery, Readable,
// SameRelease; Parse returns the attributes encoded: per set (resolution,
// quality, codec, bitDepth, audio, channels, network, edition, hdr) a u32
// count and its strings, each a u32 length and bytes; then title, group, the
// seasons and episodes (u32 count, i32 each), year (i32), repack and proper
// (one byte each); all little-endian. Names may be NULL (empty).
NzbgetRsBuf nzbget_rs_releasename_clean(const char* name, size_t len);
NzbgetRsBuf nzbget_rs_releasename_normalize(const char* name, size_t len);
NzbgetRsBuf nzbget_rs_releasename_short_query(const char* title, size_t len);
NzbgetRsBuf nzbget_rs_releasename_parse(const char* name, size_t len);
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
size_t nzbget_rs_posting_group(const NzbgetRsListing* listings, size_t n, size_t* indices, size_t* groupSizes,
	size_t* groups);
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
NzbgetRsBuf nzbget_rs_dupe_inner(const char* data, size_t len, const char* name, size_t nameLen,
    uint64_t total, uint64_t minimum);
int nzbget_rs_dupe_same_data(uint64_t aBytes, uint32_t aFiles, uint64_t bBytes, uint32_t bFiles);
int nzbget_rs_dupe_candidate(const char* title, size_t titleLen, uint64_t bytes,
    const char* name, size_t nameLen, uint64_t size);
int nzbget_rs_dupe_lenient_ok(const char* title, size_t titleLen, const char* mainName, size_t mainLen,
    const NzbgetRsDupeIdentityInput* mine, const NzbgetRsDupeIdentityInput* theirs);

// Stream repair patch selection (rust/src/streampatchparts.rs). Ranges are
// caller-owned aligned pairs of int64_t offset,size (the native StreamRange
// representation). Output is caller-owned and writable for donorCount ints;
// returns the number written, in ascending zero-based index order. No buffers
// overlap. NULL is allowed for empty slices. donorCount must fit int32_t;
// endpoint addition and original int index +/- margin overflow are outside the
// defined C++ parity domain. All sizes and offsets otherwise retain semantics.
size_t nzbget_rs_stream_patch_parts(const void* donors, size_t donorCount,
    const void* holes, size_t holeCount, int32_t margin, int32_t* output);

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
NzbgetRsBuf nzbget_rs_dupe_queries(const char* name, size_t len);
int nzbget_rs_dupe_should_expand(double alive, int32_t missing, int failed, int wholeBackup);
// An opaque per-search budget (3 * maxDonors; zero means unlimited).
// All concurrent callers must have joined before free.
void* nzbget_rs_dupe_budget_new(uint32_t maxDonors);
void nzbget_rs_dupe_budget_free(void* budget);
int nzbget_rs_dupe_budget_take(const void* budget);
int nzbget_rs_dupe_budget_spent(const void* budget);

typedef struct { int64_t number, bytes; } NzbgetRsDupePart;
void nzbget_rs_dupe_part_counts(const NzbgetRsDupePart* parts, size_t count, uint32_t declared, int parity,
    uint64_t* listed, uint64_t* total);
uint64_t nzbget_rs_dupe_whole_bytes(uint64_t bytes, uint64_t listed, uint64_t total);
int32_t nzbget_rs_dupe_unlisted_missing(int32_t answered, uint64_t listed, uint64_t total);
double nzbget_rs_dupe_effective_alive(int32_t present, int32_t missing, int32_t error, uint64_t listed, uint64_t total);
int nzbget_rs_dupe_is_dead(int32_t present, int32_t missing, int32_t error, uint64_t listed, uint64_t total, int32_t minimum);
uint64_t nzbget_rs_dupe_par_block(const char* data, size_t len);
int nzbget_rs_dupe_par_doomed(uint64_t dataBytes, uint64_t articleBytes, const uint64_t* volumes,
    size_t count, double alive, uint64_t block);

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
uint32_t nzbget_rs_memory_pressure_effective_downloads_limit(uint32_t configured_limit);
void nzbget_rs_memory_pressure_set_mock_root(const char* mock_root);
void nzbget_rs_memory_pressure_set_override(int32_t tier);
void nzbget_rs_memory_pressure_trim_heap(void);

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

typedef enum {
    NZBGET_RS_PHASE_UNINITIALIZED = 0,
    NZBGET_RS_PHASE_OPTIONS_LOGGING = 1,
    NZBGET_RS_PHASE_SUBSYSTEMS_RECOVERY = 2,
    NZBGET_RS_PHASE_SERVERS_ACTIVE = 3,
    NZBGET_RS_PHASE_SHUTDOWN_RELOAD = 4,
    NZBGET_RS_PHASE_TERMINATED = 5
} NzbgetRsLifecyclePhase;

typedef struct {
    void (*set_process_args)(int argc, char* argv[], char* argp[]);
    void (*util_init)(void);
    int  (*nserv_main)(int argc, char* argv[]);
    int  (*win_service_check)(int argc, char* argv[]);
    int  (*phase1_init)(bool reloading);
    int  (*phase2_subsystems_create_and_recover)(bool reloading);
    int  (*phase3_server_activate)(void);
    void (*phase4_shutdown)(bool reloading);
    bool (*has_active_unpack)(void);
    bool (*is_reloading)(void);
    void (*stop_server)(bool reloading);
    void (*reload)(void);
    void (*exit_proc)(void);
} NzbgetCLifecycleHooks;

int nzbget_rs_main(int argc, char* argv[], char* argp[]);
void nzbget_rs_register_lifecycle_hooks(const NzbgetCLifecycleHooks* hooks);
NzbgetRsLifecyclePhase nzbget_rs_lifecycle_get_phase(void);
void nzbget_rs_lifecycle_set_phase(NzbgetRsLifecyclePhase phase);
bool nzbget_rs_is_reloading(void);
bool nzbget_rs_is_stopping(void);
void nzbget_rs_reload(void);
void nzbget_rs_shutdown(void);
void nzbget_rs_signal_notify(int signum);

// C++ subsystem bridge functions exported to Rust
void nzbget_c_set_process_args(int argc, char* argv[], char* argp[]);
void nzbget_c_util_init(void);
int  nzbget_c_nserv_main(int argc, char* argv[]);
int  nzbget_c_win_service_check(int argc, char* argv[]);
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
    int32_t part_number;
    int32_t status;
    int64_t segment_offset;
    int32_t segment_size;
    int32_t fallback_round;
} NzbgetRsArticleGeom;

typedef struct {
    int64_t offset;
    int64_t size;
} NzbgetRsByteRange;

typedef struct {
    int32_t level;
    int32_t active;
    int32_t max_connections;
} NzbgetRsServerNormInput;

int nzbget_rs_dupe_fallback_par_cannot_cover(
    int64_t par_size, int64_t par_failed_size, int64_t current_failed_size,
    int64_t current_success_size, int64_t par_current_success_size, int64_t total_size,
    int current_success_articles, int current_failed_articles, int with_margin);
int nzbget_rs_dupe_fallback_has_extension(const char* filename);
int nzbget_rs_dupe_fallback_sizes_match(int64_t size1, int64_t size2, int div);
int nzbget_rs_dupe_fallback_declared_parts(const char* subject);
uint64_t nzbget_rs_dupe_fallback_steps_hash(const int* sizes, size_t count);
int nzbget_rs_dupe_fallback_steps_match(
    const int* target_sizes, size_t target_count,
    const int* donor_sizes, size_t donor_count);
int nzbget_rs_dupe_fallback_structure_matches(
    const char* target_subject, int target_total_articles, int64_t target_size,
    const int* target_parts, const int* target_sizes, size_t target_count,
    const char* donor_subject, int donor_total_articles, int64_t donor_size,
    const int* donor_parts, const int* donor_sizes, size_t donor_count);
int64_t nzbget_rs_dupe_fallback_expected_offset(
    ptrdiff_t index, int part_number, int prev_part_number, int prev_status,
    int64_t prev_offset, int prev_size);
int64_t nzbget_rs_dupe_fallback_expected_end(
    ptrdiff_t index, size_t count, int part_number, int64_t decoded_file_size,
    int next_part_number, int next_status, int64_t next_offset, int next_size);
int nzbget_rs_dupe_fallback_segment_aligned(
    int64_t segment_offset, int segment_size, int64_t expected_offset, int64_t expected_end);
ptrdiff_t nzbget_rs_dupe_fallback_first_untiled(
    const NzbgetRsArticleGeom* articles, size_t count, int64_t decoded_size);
int nzbget_rs_dupe_fallback_merge_decoded_size(int64_t* decoded_file_size, int64_t article_file_size);
int nzbget_rs_dupe_fallback_register_lead_failure(
    int fallback_round, int lead_snapshot, int donor_count, int next_lead,
    int* lead_donor_id, int* lead_failures, int* lead_switches);
void nzbget_rs_dupe_fallback_register_lead_success(
    int fallback_round, int lead_snapshot, int lead_donor_id,
    int64_t expected_offset, int64_t expected_end,
    int* lead_failures, int* lead_switches);
int nzbget_rs_dupe_fallback_is_par_file(
    int par_file, int first_content, int fc_par2, int filename_confirmed,
    const char* filename);
int nzbget_rs_dupe_fallback_completed_file_is_par2(
    int par_file, const char* filename);
int nzbget_rs_dupe_fallback_should_defer_to_par_header(
    int has_nzb_info, int direct_rename_status, int ts_running,
    int all_first, int par_status, int ps_failure, int64_t par_size);
int nzbget_rs_dupe_fallback_check_try_preconditions(
    int fallback_mode, int daf_none, int raw_article, int par_file,
    int first_content, int fc_par2, int fc_unknown, int part_number,
    const char* filename, int file_deleted, int nzb_deleting,
    int nzb_parking, int delete_status, int ds_none, int dupe_mode,
    int dm_force);
int nzbget_rs_dupe_fallback_eval_par_defer(
    int par_defer, int part_number, int dupe_par_defer_state,
    int dp_deferred, int dp_lifted, int par_cannot_cover,
    int damage_certain, int damage_projected,
    int* out_lift_defer_state, int* out_log_lifted);
int nzbget_rs_dupe_fallback_should_abort_deferred_try(
    int defer, int round, int has_download_queue, int source_donors_empty);
void nzbget_rs_dupe_fallback_resolve_exhausted_sources(
    int defer, int pinned_round, int dupe_proactive, int par_defer_state,
    int dp_none, int has_download_queue, int orig_msg_id_empty,
    int* out_check_log_par_defer, int* out_inc_unsourced,
    int* out_restore_orig_msg_id);
size_t nzbget_rs_dupe_fallback_find_rotate_offset(
    const int32_t* donor_ids_ptr, size_t count, int32_t lead_nzb_id);
void nzbget_rs_dupe_fallback_compute_pin_lead_state(
    const int* contributors, size_t contributors_len, int cur_file_lead_donor_id,
    int* out_lead_snapshot, int* out_next_lead,
    int* out_donor_count, int* out_new_file_lead);
int nzbget_rs_dupe_fallback_should_vacate_ghost_lead(
    int lead_nzb_id, int is_barred, const int32_t* donor_ids_ptr, size_t donor_ids_len);
int nzbget_rs_dupe_fallback_own_article_matches(
    int own_part_number, int part_number, const char* own_msg_id,
    const char* orig_msg_id, const char* cur_msg_id);
int nzbget_rs_dupe_fallback_is_donor_eligible(
    int is_self, int kind, int expected_kind, int dupe_mode, int dm_force);
int nzbget_rs_dupe_fallback_is_twin_kind(
    const char* own_identity, const char* donor_identity, const char* kind);
int nzbget_rs_dupe_fallback_compare_donors(
    int twin1, int score1, int id1, int twin2, int score2, int id2);
int nzbget_rs_dupe_fallback_should_reset_non_twin_lead(
    int twin_only, int lead_id, const char* own_identity,
    const char* lead_identity, const char* kind);
int nzbget_rs_dupe_fallback_is_bad_donor_in_cooldown(
    int64_t cur_time, int64_t bad_time, int retry_sec);
int nzbget_rs_dupe_fallback_classify_content_name_match(
    const char* donor_filename, const char* by_content);
int nzbget_rs_dupe_fallback_select_named_content_match(
    size_t exact_count, size_t folded_count);
int nzbget_rs_dupe_fallback_is_donor_filename_match(
    const char* target_filename, const char* target_nzb_filename,
    const char* donor_filename);
int nzbget_rs_dupe_fallback_resolve_structural_or_filename_match(
    int filename_ambiguous, int has_filename_match,
    size_t structural_matches_len);
int nzbget_rs_dupe_fallback_classify_first_head(
    const uint8_t* head_ptr, size_t head_len);
int nzbget_rs_par2_range_covered(
    const NzbgetRsByteRange* ranges, size_t count, int64_t from, int64_t to);
void nzbget_rs_par2_scan_file(
    const char* path, void* ctx,
    void (*on_main)(void* ctx, const char* set_id, uint64_t block_size),
    void (*on_desc)(void* ctx, const char* set_id, const char* file_id, const char* hash16k,
        uint64_t length, const char* name, size_t name_len),
    void (*on_ifsc)(void* ctx, const char* set_id, const char* file_id,
        const uint32_t* crcs, size_t crc_count));

int nzbget_rs_par_damage_blocks_touched(
    const NzbgetRsByteRange* holes, size_t count, uint64_t block_size);
void nzbget_rs_par_damage_project(
    int known, int damaged_blocks, int recovery_blocks, int64_t done_bytes, int64_t total_bytes,
    int* out_projected_blocks, int* out_certain, int* out_projected);

size_t nzbget_rs_dupe_probe_sample_indexes(
    size_t total, int count, size_t* out_indexes, size_t cap);
int nzbget_rs_dupe_probe_classify(const char* response);
int nzbget_rs_dupe_probe_is_dead(int existing, int definitive_servers, int active_servers);

int nzbget_rs_server_pool_normalize_levels(
    const NzbgetRsServerNormInput* servers, size_t count,
    size_t* out_sorted_indices, int* out_norm_levels);
int nzbget_rs_server_pool_is_blocked(int64_t block_time, int64_t cur_time, int retry_interval);
int nzbget_rs_server_pool_level_should_close(
    const uint8_t* in_use, const uint8_t* connected, const int64_t* free_times,
    size_t count, int64_t cur_time, int hold_seconds);

int nzbget_rs_article_fetch_add_raw_bytes(int64_t* raw_bytes, int64_t bytes);
int nzbget_rs_article_fetch_header_within_limit(int64_t raw_bytes, int declared_range_known);
int nzbget_rs_article_fetch_add_decoded_bytes(
    int64_t* decoded_bytes, int64_t bytes, int64_t begin, int64_t end, int64_t file_size);

int nzbget_rs_article_downloader_check_response(const char* response, int auth_error);
int nzbget_rs_article_downloader_validate_yenc(
    int64_t begin_pos, int64_t end_pos, int64_t article_file_size, int64_t expected_file_size,
    int fallback_round, int64_t dupe_expected_offset, int64_t dupe_expected_end,
    int64_t* out_offset, int* out_size);

typedef struct NzbgetRsMemberRange {
    int32_t member_index;
    int64_t offset;
    int64_t size;
} NzbgetRsMemberRange;

typedef struct NzbgetRsContentRun {
    int64_t inner_offset;
    int32_t member_index;
    int64_t member_offset;
    int64_t size;
} NzbgetRsContentRun;

int nzbget_rs_stream_is_eligible(const char* filename);
size_t nzbget_rs_stream_compute_holes(
    const NzbgetRsByteRange* covered, size_t count, int64_t file_size,
    NzbgetRsByteRange* out_holes, size_t cap);
int64_t nzbget_rs_stream_total_size(const NzbgetRsByteRange* ranges, size_t count);
int nzbget_rs_stream_intersects_any(
    NzbgetRsByteRange range, const NzbgetRsByteRange* ranges, size_t count);
size_t nzbget_rs_stream_estimate_donor_ranges(
    const int* article_sizes, size_t count, int64_t target_file_size,
    NzbgetRsByteRange* out_ranges);
size_t nzbget_rs_stream_select_probe_parts(
    const NzbgetRsByteRange* donor_ranges, size_t donor_count,
    const NzbgetRsByteRange* holes, size_t hole_count, int count,
    int* out_parts, size_t cap);
size_t nzbget_rs_stream_subtract_covered(
    const NzbgetRsByteRange* holes, size_t hole_count, NzbgetRsByteRange covered,
    NzbgetRsByteRange* out_holes, size_t cap);
NzbgetRsBuf nzbget_rs_stream_volume_set_key(const char* filename);
int nzbget_rs_stream_decoded_size_plausible(int64_t decoded_size, int64_t encoded_size);
NzbgetRsBuf nzbget_rs_stream_suffix_key(const char* filename);
NzbgetRsBuf nzbget_rs_stream_volume_key(const char* filename);
int nzbget_rs_stream_exceeds_decompress_cap(
    int64_t total_bytes, int64_t next_chunk, int64_t total_extent, int64_t member_declared_size);

size_t nzbget_rs_composite_to_members(
    const int32_t* member_indexes, const int64_t* disk_bases, const int64_t* member_sizes,
    size_t member_count, NzbgetRsByteRange range, NzbgetRsMemberRange* out_pieces, size_t cap);
int nzbget_rs_holed_source_intersects(
    const NzbgetRsByteRange* holes, size_t hole_count, int64_t offset, int64_t size);
size_t nzbget_rs_content_map_to_inner(
    const NzbgetRsContentRun* runs, size_t run_count, int32_t member_index,
    NzbgetRsByteRange member_range, NzbgetRsByteRange* out_ranges, size_t cap);
size_t nzbget_rs_content_map_from_inner(
    const NzbgetRsContentRun* runs, size_t run_count, NzbgetRsByteRange inner_range,
    NzbgetRsMemberRange* out_pieces, size_t cap);
size_t nzbget_rs_content_map_cipher_range(
    const NzbgetRsContentRun* runs, const int64_t* cipher_data_offsets,
    const int64_t* cipher_sizes, const uint8_t* has_crypto, size_t run_count,
    NzbgetRsByteRange cipher_range, NzbgetRsMemberRange* out_pieces, size_t cap);
void nzbget_rs_content_mapper_group_sets(
    const char* const* member_names, size_t count, void* ctx,
    void (*on_set)(void* ctx, int32_t format, const int32_t* members, size_t member_count));
size_t nzbget_rs_content_mapper_coalesce_ranges(
    const NzbgetRsByteRange* ranges, size_t count, NzbgetRsByteRange* out_ranges, size_t cap);
int nzbget_rs_content_mapper_build_zip_map(
    const int32_t* member_indexes, const int64_t* member_sizes, size_t count,
    void* ctx,
    int (*read_cb)(void* ctx, int32_t member_index, int64_t offset, uint8_t* buf, int64_t size),
    void (*on_success)(void* ctx, const char* inner_name, int64_t inner_size,
        const NzbgetRsContentRun* runs, size_t run_count),
    void (*on_error)(void* ctx, const char* skip_reason));
int nzbget_rs_content_mapper_build_seven_zip_map(
    const int32_t* member_indexes, const int64_t* member_sizes, size_t count,
    void* ctx,
    int (*read_cb)(void* ctx, int32_t member_index, int64_t offset, uint8_t* buf, int64_t size),
    void (*on_success)(void* ctx, const char* inner_name, int64_t inner_size,
        const NzbgetRsContentRun* runs, size_t run_count),
    void (*on_error)(void* ctx, const char* skip_reason));

size_t nzbget_rs_stream_build_probe_windows(
    int64_t inner_size, const NzbgetRsByteRange* holes, size_t hole_count,
    const NzbgetRsByteRange* original_holes, size_t orig_count,
    int probe_count, int64_t min_probe_compare_bytes,
    NzbgetRsByteRange* out_windows, size_t cap);
int64_t nzbget_rs_stream_plain_compare_floor(
    const NzbgetRsContentRun* runs, size_t run_count,
    const NzbgetRsByteRange* original_inner_holes, size_t orig_count,
    const NzbgetRsByteRange* windows, size_t window_count);
int64_t nzbget_rs_stream_encrypted_chain_start(
    int64_t window_offset, int64_t window_end, int64_t block_size,
    const NzbgetRsByteRange* original_holes, size_t orig_count);
int nzbget_rs_stream_buffer_has_varied_data(const uint8_t* data, size_t len);
void nzbget_rs_stream_compute_positional_rank(
    const char* const* member_names, const int64_t* member_sizes, size_t member_count,
    const char* target_filename, int64_t target_decoded_size,
    int* out_rank, int* out_window);

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

int nzbget_rs_dupecoord_same_name_or_key(
    const char* name1, const char* dupe_key1, const char* name2, const char* dupe_key2);
int nzbget_rs_dupecoord_failover_warranted(int item_score, int health, int backup_score);
int nzbget_rs_dupecoord_parse_alive_permille(const char* value);
int nzbget_rs_dupecoord_fails_over(int dupe_check, int dupe_mode, int health_check);
NzbgetRsBuf nzbget_rs_dupecoord_format_no_backup_reason(
    int total, int not_backup, int forced, int no_file, int unhealthy, int bad, int dead);
int nzbget_rs_dupecoord_status_from_text(const char* text_status);
int nzbget_rs_dupecoord_select_dupe_backup(
    int has_self_nzb, uint32_t self_full_content_hash, int dupe_health,
    const NzbgetRsDupeHistoryCandidate* history, size_t history_count,
    const NzbgetRsDupeQueueCandidate* queue, size_t queue_count,
    void* ctx, int (*file_exists_cb)(void* ctx, size_t history_index));

int nzbget_rs_dupematcher_size_diff_ok(int64_t size1, int64_t size2, int max_diff_percent);
int nzbget_rs_dupematcher_parse_rar_lister_line(
    const char* text, int64_t expected_size, int64_t* max_size, int* compressed, int* last_size_max);

size_t nzbget_rs_repair_select_more_pars(
    const int* sorted_block_counts, size_t count, int block_needed,
    size_t* out_indices, size_t cap, int* out_remaining_needed);
ptrdiff_t nzbget_rs_repair_pick_twin_volume(
    const int* sorted_volume_blocks, size_t count, int left);
int nzbget_rs_repair_check_par_time_limit(
    int par_time_limit_min, int is_repairing_stage, int64_t elapsed_sec,
    int stage_progress, int* out_estimated_sec);

void nzbget_rs_parchecker_calc_repairer_resources(
    int par_threads, int cpu_cores, int has_memory_monitor, int monitor_mem_mb,
    int configured_par_buffer_mb, int* out_threads, int* out_mem);
int nzbget_rs_parchecker_build_preprocess_cli_args(
    int threads_to_use, int mem_to_use, const char* par_filename, int par_scan_full,
    NzbgetRsBuf* out_mem_param, NzbgetRsBuf* out_threads_param,
    NzbgetRsBuf* out_file_threads_param, NzbgetRsBuf* out_wildcard_param);
const char* nzbget_rs_parchecker_cmdline_err_str(int res);
const char* nzbget_rs_parchecker_stage_message(int stage);
NzbgetRsBuf nzbget_rs_parchecker_format_action_label(const char* verb, const char* info_name);
int nzbget_rs_parchecker_should_try_more_recovery(
    int has_damaged_files, int is_stopped, int res, int e_repair_not_possible,
    int require_dupe_scan, int par_scan, int ps_dupe);
int nzbget_rs_parchecker_should_add_missing_files(
    int has_damaged, int is_stopped, uint32_t missing_file_count,
    int added_split, int res, int e_repair_possible,
    int par_scan, int ps_extended, int ps_dupe);
int nzbget_rs_parchecker_eval_post_verify_action(
    int res, int e_success, int e_repair_possible, int has_damaged_files,
    int par_repair, int ps_failed, int ps_repair_not_needed, int ps_repair_possible,
    int* out_action_code);
int nzbget_rs_parchecker_eval_cancel_or_fail_outcome(
    int is_stopped, int stage, int pt_repairing, int cur_status,
    int ps_failed, int ps_repair_possible, int err_msg_empty, int res,
    int* out_outcome_kind, int* out_use_cmd_err);
int nzbget_rs_parchecker_classify_preprocess_step(
    int is_stopped, int res, int e_success, int e_invalid_args);
NzbgetRsBuf nzbget_rs_parchecker_format_not_enough_blocks_errmsg(
    int32_t missing_blocks, int32_t block_found);
NzbgetRsBuf nzbget_rs_parchecker_format_twin_err_msg(
    int res, int e_repair_not_possible, const char* own_err_msg);
int nzbget_rs_parchecker_maybe_splitted_fragment(const char* filename1, const char* filename2);
int nzbget_rs_parchecker_calc_dupe_blocks_delta(
    int prev_used_blocks, uint32_t was_blocks_missing2,
    uint32_t blocks_missing2, uint32_t was_blocks_missing_total,
    uint32_t* out_total_found);
NzbgetRsBuf nzbget_rs_parchecker_extract_base_par_stem(const char* par_filename);
int nzbget_rs_parchecker_compare_extra_file_stems(
    const char* base_name1, const char* base_name2, const char* par_stem);
int nzbget_rs_parchecker_eval_extra_file_step(
    uint32_t was_files_missing, uint32_t now_files_missing,
    uint32_t was_blocks_missing, uint32_t now_blocks_missing,
    int external_dir, int prev_files_added, int only_missing,
    int* out_file_added, int* out_block_added,
    uint32_t* out_blocks_delta, int* out_register_parred, int* out_abort_reason);
int nzbget_rs_parchecker_calc_stage_progress(
    int stage, int file_progress, int processed_count, int files_to_repair,
    int source_files_count, int extra_files, int quick_files);
int nzbget_rs_parchecker_eval_signal_done(
    int stage, int pt_verifying_sources, int available, int total,
    int verifying_extra_files, int ignore, int prev_has_damaged_files,
    int* out_has_damaged_files, int* out_bad_blocks);
int nzbget_rs_parchecker_check_verify_data_file_preconditions(
    int stage, int pt_verifying_sources, int target_exists,
    int has_verification_packet, int64_t file_size, uint64_t block_count);
uint32_t nzbget_rs_parchecker_combine_block_crcs(
    const uint32_t* block_crcs_ptr, size_t count, uint64_t block_size);
void nzbget_rs_parchecker_compute_valid_blocks(
    size_t block_count, int64_t block_size, int64_t file_size,
    const NzbgetRsParSegment* segments, size_t segment_count, uint8_t* out_valid);
int32_t nzbget_rs_parchecker_next_valid_block_run(
    const uint8_t* valid_blocks_ptr, const uint32_t* block_crcs_ptr,
    size_t len, size_t start_idx, int64_t block_size, int64_t file_size,
    size_t* out_block_end, uint32_t* out_par_crc,
    int64_t* out_bytes_start, int64_t* out_clamped_end, size_t* out_pad_len);
int nzbget_rs_parchecker_smart_calc_range_crc(
    int64_t start, int64_t end, const NzbgetRsParSegment* segments, size_t segment_count,
    void* ctx, int (*dumb_calc_cb)(void* ctx, int64_t start, int64_t end, uint32_t* out_crc),
    uint32_t* out_crc);
int nzbget_rs_parchecker_classify_streambuf_line(const char* msg, size_t* out_offset);

int nzbget_rs_unpack_detect_rar_signature(const uint8_t* sig_bytes, size_t sig_len);
int nzbget_rs_unpack_check_joinable_fragments(
    int count, int min_seg, int max_seg, int dif_size_min, int par_success);
void nzbget_rs_unpack_parse_message(
    const char* text, const char* unpack_extended_dir, int is_seven_zip,
    NzbgetRsUnpackMessageInfo* out_info);

// ============================================================================
// QueueCoordinator, ArticleWriter, HistoryCoordinator, QueueEditor &
// DownloadInfo Hot-Path FFI (`rust/src/queuehotpath.rs`)
// ============================================================================

typedef struct NzbgetRsCandidateFile {
    int32_t nzb_priority;
    int32_t extra_priority;
    int32_t paused;
    int32_t deleted;
    int32_t already_checked;
    int64_t post_time;
} NzbgetRsCandidateFile;

typedef struct NzbgetRsDeadDownloadEval {
    int32_t own;
    int32_t tried;
    int32_t dead_rate;
    int32_t dead_run;
    int32_t dead;
    int32_t projected;
    int32_t doomed;
    int32_t hopeless;
    int32_t far_below;
    int32_t below;
} NzbgetRsDeadDownloadEval;

int nzbget_rs_qhp_calc_health(
    int64_t size, int64_t par_size, int64_t current_failed_size, int64_t par_current_failed_size);
int nzbget_rs_qhp_calc_critical_health(
    int64_t size, int64_t par_size, int64_t par_current_failed_size, int allow_estimation);

int nzbget_rs_qhp_calc_write_buffer_size(int write_buffer_kb, int rec_size);
int nzbget_rs_qhp_classify_completed_file_status(
    int total_articles, int success_articles, int missed_articles, int failed_articles);
NzbgetRsBuf nzbget_rs_qhp_resolve_completed_filename(
    int completed, int file_status, const char* output_filename, const char* file_filename,
    const char* dest_dir, char path_sep, int output_file_exists);
int nzbget_rs_qhp_article_cache_should_flush(
    size_t allocated, size_t effective_limit, int tier, int just_flushed, int reset_counter,
    int is_stopped, int direct_write, int* out_flush_everything, int* out_need_emergency_flush);
int nzbget_rs_qhp_memory_monitor_step(
    int raw_sample, int current_tier, int* candidate_tier, int* consecutive_samples,
    int* out_next_tier);

int nzbget_rs_qhp_adjust_downloads_limit(
    const int32_t* norm_levels, const uint8_t* actives, const int32_t* max_conns, size_t count);
int nzbget_rs_qhp_is_propagation_wait(int64_t post_time, int propagation_delay, int64_t cur_date);
void nzbget_rs_qhp_select_next_file(
    const NzbgetRsCandidateFile* files, size_t count, int propagation_delay, int64_t cur_date,
    int* out_best_idx, int* out_prop_wait_idx);
int nzbget_rs_qhp_is_data_filename(const char* filename);
int nzbget_rs_qhp_size_near(int64_t size1, int64_t size2);
int nzbget_rs_qhp_download_hopeless(int total, int current_success, int current_failed);
void nzbget_rs_qhp_eval_dead_download(
    int current_success, int dupe_recovered, int current_failed, int file_count,
    int dupe_failed_run, int par_cannot_cover_exact, int critical, int par_edge_margin,
    int files_tried, int probing_sec, NzbgetRsDeadDownloadEval* out_eval);
int nzbget_rs_qhp_is_side_file(const char* filename, const char* ext_cleanup_disk);
int nzbget_rs_qhp_is_hanging_download(
    int64_t now, int64_t last_update, int article_timeout, int is_running);

int nzbget_rs_qhp_should_cleanup_parked_files(
    int par_status, int unpack_status, int64_t failed_size, int64_t par_failed_size,
    int delete_status, int parking, int unpack_cleaned_up_disk);
int nzbget_rs_qhp_classify_dup_status(int mark_status, int delete_status, int is_dupe_success);
int nzbget_rs_qhp_should_reset_article(
    int article_status, int partial_state, int all_failed, int reset_failed);
int nzbget_rs_qhp_parse_dupe_mode(const char* text, int* out_mode);

int nzbget_rs_qhp_parse_sort_spec(const char* sort, int* out_criteria, int* out_order);
void nzbget_rs_qhp_clamp_move_offset(
    int index, int nr_entries, int offset, int last_dest_pos,
    int* out_work_offset, int* out_last_dest_pos);
void nzbget_rs_qhp_select_pars_to_pause(
    const uint8_t* is_par, const uint8_t* has_vol, const int64_t* sizes, size_t len,
    int extra_pars_only, uint8_t* out_pause);
int nzbget_rs_qhp_compare_inner_files(
    const char* name1, int par1, int64_t size1,
    const char* name2, int par2, int64_t size2, int same_par_collection);

// ============================================================================
// DiskState, PrePostProcessor, Scanner & TwinCheck Hot-Path FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

typedef struct NzbgetRsArticleLoadEval {
    int32_t valid;
    int32_t status;
    int64_t segment_offset;
    int32_t segment_size;
    int32_t dupe_fallback_round;
    int32_t need_result_filename;
    int32_t completed_articles;
} NzbgetRsArticleLoadEval;

typedef struct NzbgetRsPostJobCandidate {
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
} NzbgetRsPostJobCandidate;

typedef struct NzbgetRsPostStageDecision {
    int32_t unpack;
    int32_t skip_unpack_due_to_par;
    int32_t cleanup;
    int32_t move_inter;
    int32_t post_download_renaming;
    int32_t post_unpack_renaming;
} NzbgetRsPostStageDecision;

int nzbget_rs_dsp_parse_format_version(const char* format_signature);
void nzbget_rs_dsp_sanitize_line_breaks(char* buf, size_t len);
int nzbget_rs_dsp_parse_ids_file(const char* text, int* out_nzb_id, int* out_file_id);
int nzbget_rs_dsp_classify_queue_dir_id(const char* filename, int* out_is_nzb_log);
int nzbget_rs_dsp_classify_legacy_file_state(
    const char* filename, const char* header_line, int file_version, int legacy_version,
    int* out_id, int* out_suffix_kind);
int nzbget_rs_dsp_validate_repair_hole(
    uint32_t offset_hi, uint32_t offset_lo, uint32_t size_hi, uint32_t size_lo,
    int64_t decoded_size, int64_t* out_offset, int64_t* out_size);
int nzbget_rs_dsp_has_dupe_extras(
    int recovered_articles, int64_t recovered_bytes, int recovered_holes, size_t repair_jobs_count);
int nzbget_rs_dsp_pack_unpack_tried_flags(int unpack_tried, int par_repaired);
int nzbget_rs_dsp_parse_unpack_state_lines(
    const char* line1, const char* line2, const char* line3, int ns_running, int ns_none,
    int* out_direct_status, int* out_unpack_tried, int* out_par_repaired, int* out_last_unpack_status);
int nzbget_rs_dsp_normalize_post_stage(int post_stage, int format_version, int pt_finished);
int nzbget_rs_dsp_eval_article_load_state(
    int format_version, int completed, int fields, int status_in, int64_t segment_offset_in,
    int segment_size_in, int staged_fallback_in, int part_number, int staged_file_exists,
    int direct_write, int force_direct_write, int has_result_filename, int completed_articles_in,
    int total_size, NzbgetRsArticleLoadEval* out_eval);
void nzbget_rs_dsp_reconcile_direct_write_output(
    int output_initialized, int has_own_out_file, int has_out_tmp, int has_completed_dest,
    int success_articles, int completed, int output_filename_empty,
    int* out_new_output_initialized, int* out_pick_path_kind);
int nzbget_rs_dsp_classify_temp_file(const char* filename, int* out_file_id, int* out_part);
int nzbget_rs_dsp_is_orphan_queue_file(
    const char* filename, const int* sorted_file_ids, size_t file_ids_len,
    const int* sorted_nzb_ids, size_t nzb_ids_len);
int nzbget_rs_dsp_match_servers(
    const char* const* server_names, const char* const* server_hosts, const int* server_ports,
    const char* const* server_users, size_t server_count,
    const int* ref_state_ids, const char* const* ref_names, const char* const* ref_hosts,
    const int* ref_ports, const char* const* ref_users, size_t ref_count,
    int* out_server_state_ids);
void nzbget_rs_dsp_sanitize_log_text(char* buf, size_t len);
int nzbget_rs_dsp_parse_log_line(
    const char* line, int* out_time, int* out_kind, size_t* out_text_offset);

void nzbget_rs_prepost_sanitise_post_stage(
    int stage, int pt_executing_script, int pt_finished, int pt_queued, int dest_dir_exists,
    int unpack_tried, int par_status, int ps_skipped, int* out_stage, int* out_request_par_check);
int nzbget_rs_prepost_classify_file_event(
    int is_file_completed, int is_file_deleted, int dupe_deleted, int delete_status,
    int ds_none, int ds_health, int has_post_info, int download_completed_ignore_paused,
    int download_completed_all);
int nzbget_rs_prepost_should_complete_after_direct_rename(
    int is_stopped, int direct_rename_status, int ts_success, int has_post_info,
    int delete_status, int ds_none, int completed_not_empty, int download_completed,
    int raw_article, int skip_write, int skip_disk_write);
int nzbget_rs_prepost_classify_nzb_downloaded_entry(
    int skip_script_processing, int skip_disk_write, int opt_skip_write,
    int delete_status, int ds_health, int ds_bad, int has_post_info,
    int opt_raw_article, int par_status, int ps_none, int par_check_opt,
    int pc_always, int pc_force,
    int* out_enqueue_deleted_script, int* out_set_par_skipped);
int nzbget_rs_prepost_resolve_nzb_deleted_route(
    int delete_status, int ds_none, int ds_manual, int ds_health, int ds_bad,
    int* out_new_delete_status);
void nzbget_rs_prepost_eval_nzb_completed(
    int dupe_hint, int dh_redownload_auto, int keep_history, int avoid_history, int dupe_check,
    int dupe_mode, int dm_force, int fails_over, int delete_status, int ds_none, int ds_health,
    int ds_bad, int ds_scan, int ds_copy, int* out_add_to_history, int* out_call_dupe_completed,
    int* out_enqueue_deleted_script, int* out_need_save);
int nzbget_rs_prepost_classify_delete_cleanup_mode(
    int cleanup_disk, int delete_status, int ds_dupe, int parking,
    int skip_disk_write, int opt_skip_write);
int nzbget_rs_prepost_classify_request_par_action(
    int request_par_check, int par_status, int ps_skipped, int force_repair,
    int par_full, int par_check_opt, int pc_manual, int cur_force_par_full,
    int file_list_empty, int* out_new_force_par_full);
int nzbget_rs_prepost_eval_cleanup_job_state(
    int working, int has_running_thread, int stage, int pt_finished, int deleted,
    int64_t start_time, int64_t cur_time, int prev_post_total_sec,
    int* out_update_total_sec, int* out_is_completed, int* out_new_total_sec);
int nzbget_rs_prepost_can_run_more_jobs(
    const int* job_stages, size_t job_count, int pt_loading_pars, int pt_verifying_repaired,
    int pt_repairing, int post_strategy, int* out_allow_par);
int nzbget_rs_prepost_pick_next_post_job(
    const NzbgetRsPostJobCandidate* candidates, size_t count, int allow_par,
    int has_active_unpack, int pause_post_process);
int nzbget_rs_prepost_classify_skipped_par_health(
    int par_status, int ps_skipped, int par_scan_opt, int ps_dupe,
    int health, int critical_health, int64_t failed_size, int64_t par_failed_size);
int nzbget_rs_prepost_eval_want_unpack_and_rar_rename(
    int has_unpack_param, const char* unpack_param_val,
    int unpack_status, int us_none, int delete_status, int ds_none,
    int rar_rename_status, int rs_none, int opt_rar_rename,
    int* out_want_unpack, int* out_unpack);
void nzbget_rs_prepost_eval_post_stages(
    int want_unpack, int unpack_status, int delete_status, int par_status, int cleanup_status,
    int has_ext_cleanup_disk, int health, int move_status,
    const char* dest_dir, const char* inter_dir, char sep,
    int direct_unpack_status, int opt_post_download_rename, int post_download_renaming_status,
    int has_dest_dir, int has_name, int opt_rename_after_unpack, int post_unpack_renaming_status,
    NzbgetRsPostStageDecision* out_decision);
int nzbget_rs_prepost_calc_post_pause_state(
    const int* job_stages, size_t job_count, int par_pause_queue, int unpack_pause_queue,
    int script_pause_queue);
int nzbget_rs_prepost_classify_file_downloaded_direct_unpack(
    int opt_direct_unpack, int opt_raw_article, int opt_skip_write,
    int skip_disk_write, int direct_unpack_status, int ns_none, int ns_running,
    int direct_rename_status, int ts_running, int has_unpack_thread);

void nzbget_rs_dsp_scanner_next_pass(
    int nzb_dir_interval, int nzb_dir_file_age, int has_scan_script, int current_pass,
    int* out_next_interval, int* out_next_pass);
int nzbget_rs_dsp_can_process_extension(const char* full_filename);
int nzbget_rs_dsp_eval_file_age(
    int64_t old_size, int64_t new_size, int64_t last_change, int64_t current_time, int file_age,
    int64_t* out_size, int64_t* out_last_change);
int nzbget_rs_dsp_should_drop_scan_file(
    int64_t last_change, int64_t current_time, int nzb_dir_interval, int nzb_dir_file_age);
int nzbget_rs_dsp_classify_missed_articles(
    int missed_articles, int total_articles, int is_cut_short);
int nzbget_rs_dsp_is_nzb_xml_prefix(const uint8_t* buf, size_t len);
NzbgetRsBuf nzbget_rs_dsp_normalize_external_nzb_name(
    const char* nzb_name, const char* valid_name, int is_nzb);

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

typedef struct NzbgetRsPendingMetaView {
    int32_t valid;
    size_t title_offset;
    size_t title_len;
    size_t indexer_offset;
    size_t indexer_len;
    int32_t grabs;
    int64_t size;
    int64_t date;
} NzbgetRsPendingMetaView;

typedef struct NzbgetRsDupeSearchStateLine {
    int32_t valid;
    int32_t within_ttl;
    size_t key_offset;
    size_t key_len;
    int32_t score;
    int64_t time;
    size_t title_offset;
    size_t title_len;
    int32_t retryable;
} NzbgetRsDupeSearchStateLine;

NzbgetRsBuf nzbget_rs_df_lower_key(const char* key);
int nzbget_rs_df_saved_history_backup(
    int has_pick, uint32_t pick_full_hash, int candidate_delete_status,
    uint32_t candidate_full_hash, int ds_dupe, int ds_copy);
int nzbget_rs_df_placement_available(
    int has_backup, int is_nzb_kind, int deleting, int delete_status,
    int ds_none, int ds_manual, int ds_dupe, int ds_copy,
    int mark_status, int ks_bad, int dupe_mode, int dm_force,
    int in_history, int whole_backup, int has_post_info, int is_dupe_success,
    int has_pick, uint32_t pick_full_hash, uint32_t backup_full_hash,
    int queued_file_exists, int health, int critical_health,
    int health_check_is_dupe, int has_marked_alive, int marked_alive_val);
void nzbget_rs_df_sort_verified_postings(
    const int64_t* total_bytes, const int* grabs, const int64_t* dates,
    size_t count, int64_t pick_bytes, size_t* out_order);
void nzbget_rs_df_eval_member_rerank(
    int base, int wanted_score, int dead_score, int old_score, double alive, int all_dead,
    int* out_score, int* out_percent, int* out_add_alive_zero);
int nzbget_rs_df_eval_pick_gone(
    int has_pick, int deleting, int delete_status, int ds_manual,
    const char* pick_eff_key, const char* job_dupe_key,
    const char* pick_norm_name, const char* job_norm_name);
NzbgetRsBuf nzbget_rs_df_format_pending_meta(
    const char* title, const char* indexer, int grabs, int64_t size, int64_t date);
void nzbget_rs_df_parse_pending_meta(
    const char* meta, size_t len, NzbgetRsPendingMetaView* out_view);
int nzbget_rs_df_parse_dupesearch_state_line(
    const char* line, size_t len, int64_t now, int64_t state_ttl_sec,
    NzbgetRsDupeSearchStateLine* out_line);

void nzbget_rs_df_eval_fleet_member_health(
    int checked, int result_complete, int present, int missing, int error,
    int min_known, int min_alive_pct,
    int* out_alive_pct, int* out_dead, int* out_member_complete);
size_t nzbget_rs_df_rank_fleet_candidates(
    const uint8_t* has_error, const int* same_as, const uint8_t* dead,
    const int* alive, const int64_t* total_bytes, size_t count, size_t* out_ranked);
int nzbget_rs_df_eval_fleet_outcome_reason(
    int any_usable, int any_alive, int any_unknown, int on_disk);

double nzbget_rs_df_donor_health_alive(
    int present, int missing, int error, int min_known);
int nzbget_rs_df_donor_health_dead_probe(
    int present, int missing, int min_known);
size_t nzbget_rs_df_donor_sample_count(
    size_t total_ids, int percent, int minimum, int maximum);
int nzbget_rs_df_adjust_pipeline_depth(
    int depth, int64_t took_ms, int pipeline_max);
int nzbget_rs_df_eval_article_votes(const int* votes, size_t count);
int nzbget_rs_df_resolve_unsettled_vote(const int* votes, size_t count);

int64_t nzbget_rs_df_parse_newznab_date(const char* text, size_t len);
NzbgetRsBuf nzbget_rs_df_build_newznab_url(
    const char* base, const char* const* param_keys, const char* const* param_vals,
    size_t param_count, const char* api_key);
NzbgetRsBuf nzbget_rs_df_mask_newznab_text(const char* text, size_t len);

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================

typedef struct NzbgetRsDirectRenameParDelta {
    int32_t apply_hash16k;
    int32_t par_flag_changed;
    int64_t par_size;
    int64_t par_current_success_size;
    int64_t par_current_failed_size;
    int64_t par_failed_size;
    int32_t remaining_par_count;
    int32_t unpause_non_par;
} NzbgetRsDirectRenameParDelta;

typedef struct NzbgetRsQueueScriptCmd {
    int32_t kind;
    size_t key_offset;
    size_t key_len;
    size_t val_offset;
    size_t val_len;
} NzbgetRsQueueScriptCmd;

typedef struct NzbgetRsPostScriptCmd {
    int32_t kind;
    size_t key_offset;
    size_t key_len;
    size_t val_offset;
    size_t val_len;
} NzbgetRsPostScriptCmd;

typedef struct NzbgetRsScanScriptCmd {
    int32_t kind;
    size_t key_offset;
    size_t key_len;
    size_t val_offset;
    size_t val_len;
    int32_t int_val;
} NzbgetRsScanScriptCmd;

int nzbget_rs_sus_eval_interval_check(
    int64_t cur_time, int64_t last_check, int64_t last_time_offset,
    int64_t start_download, int stand_by,
    int64_t* out_clock_delta, int* out_adjust_download, int* out_adjust_time_offset);
void nzbget_rs_sus_calc_total_stat(
    int64_t cur_time, int64_t start_server, int64_t start_download,
    int64_t paused_from, int stand_by,
    int* out_up_time_sec, int* out_dn_time_sec);
int64_t nzbget_rs_sus_calc_current_download_speed(
    int stand_by, int64_t cur_time, int speed_start_time, int64_t speed_total_bytes);
int64_t nzbget_rs_sus_calc_momentary_download_speed(
    int64_t cur_time, int64_t cur_sec_time, int64_t cur_sec_bytes);
void nzbget_rs_sus_add_speed_reading(
    int bytes, int64_t cur_time, int64_t* cur_sec_time, int64_t* cur_sec_bytes,
    int32_t* speed_bytes, int32_t* speed_time, int* speed_bytes_index,
    int64_t* speed_total_bytes, int* speed_start_time,
    int64_t* speed_correction, int64_t* all_bytes);
void nzbget_rs_sus_calc_quota_usage(
    int64_t loc_time, int first_day, const int64_t* bytes_per_days, size_t days_len,
    int elapsed_slots, int64_t* out_month_bytes, int64_t* out_day_bytes);
int nzbget_rs_sus_eval_quota_status(
    int monthly_quota_mb, int daily_quota_mb, int64_t month_bytes, int64_t day_bytes,
    int was_quota_reached, int* out_action);

int nzbget_rs_sus_web_classify_get_dispatch(const char* url);
int nzbget_rs_sus_web_is_ignorable_404(const char* base_name);
const char* nzbget_rs_sus_web_detect_content_type(const char* filename);

int nzbget_rs_sus_parse_queue_script_cmd(
    const char* msg_text, int event, int qe_nzb_downloaded,
    NzbgetRsQueueScriptCmd* out_cmd);
int nzbget_rs_sus_should_skip_queue_event_interval(
    int event, int qe_file_downloaded, int event_interval,
    int64_t cur_time, int64_t queue_script_time);
int nzbget_rs_sus_is_usable_queue_script(
    int is_queue_script, const char* queue_events, const char* script_name,
    int event, int qe_url_completed,
    const char* const* param_names, const char* const* param_values,
    size_t param_count, const char* url_extensions);
int nzbget_rs_sus_should_ignore_queued_script_event(
    int has_nzb_info, int delete_status, int ds_none,
    int mark_status, int ks_bad, int event,
    int qe_nzb_deleted, int qe_nzb_marked);

NzbgetRsBuf nzbget_rs_sus_build_enabled_script_comma_list(
    const char* const* param_names, const char* const* param_values,
    size_t param_count);
int nzbget_rs_sus_analyse_post_script_exit_code(
    int exit_code, int par_status, int ps_skipped, int parcheck_enabled,
    int sr_none, int sr_failure, int sr_success,
    int* out_message_action, int* out_request_par_check);
int nzbget_rs_sus_parse_post_script_cmd(
    const char* msg_text, NzbgetRsPostScriptCmd* out_cmd);

int nzbget_rs_sus_parse_scan_script_cmd(
    const char* msg_text, NzbgetRsScanScriptCmd* out_cmd);

NzbgetRsBuf nzbget_rs_sus_build_cipher_baseline(
    const char* const* names, const char* const* values, size_t count);

int nzbget_rs_sus_is_nzbget_internal_env_var(const char* var);
NzbgetRsBuf nzbget_rs_sus_normalize_special_env_var_name(const char* varname);
int nzbget_rs_sus_classify_script_output_line(
    char* line, int* out_kind, size_t* out_offset);
int nzbget_rs_sus_check_env_size_limit(
    const char* const* env_vars, size_t count, size_t ptr_size, size_t limit,
    size_t* out_exceeded_total);

int nzbget_rs_rco_rpc_is_rpc_request(const char* url);
void nzbget_rs_rco_rpc_log_slice_range(
    int messages_len, int front_id, int id_from, int nr_entries,
    int* out_start, int* out_nr_entries);
int nzbget_rs_rco_rpc_list_files_match_and_progress(
    int nzb_id, int file_nzb_id, int file_id, int id_start, int id_end,
    int64_t size, int64_t remaining_size, int64_t missed_size,
    int64_t failed_size, int64_t success_size, int* out_progress);
const char* nzbget_rs_rco_rpc_detect_group_status(
    int has_post_info, int post_stage, int has_queue_script_job,
    int queue_script_active, int direct_unpack_status, int active_downloads,
    int kind, int64_t paused_size, int64_t remaining_size);
int nzbget_rs_rco_rpc_lookup_edit_command(const char* edit_command);
int nzbget_rs_rco_rpc_validate_edit_queue_args(
    int action, const char* args, int parsed_int_ok);
int nzbget_rs_rco_rpc_parse_write_log_kind(const char* kind);
const char* nzbget_rs_rco_rpc_detect_history_dup_status(int dup_status);
int nzbget_rs_rco_rpc_parse_update_branch(const char* branch_name);
int nzbget_rs_rco_rpc_save_config_parse_bool(const char* val, int def_val);
int nzbget_rs_rco_rpc_save_config_check_global_conflicts(
    const char* dupe_search_val, const char* health_check_val,
    const char* flush_queue_val, const char* direct_write_val,
    const char* unpack_pause_queue_val, const char* direct_unpack_val);

int nzbget_rs_rco_remote_classify_signature(
    const char* sig_ptr, uint32_t nzb_magic_be);
int nzbget_rs_rco_remote_parse_web_request_line(
    const char* sig_ptr, const char* line, int* out_http_method,
    size_t* out_url_offset, size_t* out_url_len);

int nzbget_rs_rco_conn_read_line_chunk(
    const char* buf_ptr, int buf_avail, int max_out, int* out_found_newline);
int nzbget_rs_rco_conn_classify_accept_errno(
    int err, int eintr, int econnaborted, int eproto, int enetdown,
    int enoprotoopt, int ehostdown, int ehostunreach, int eopnotsupp,
    int enetunreach, int enonet, int emfile, int enfile, int enobufs, int enomem);

int nzbget_rs_rco_webdl_resolve_protocol_port(
    int is_valid, const char* protocol, int port, int tls_disabled,
    int* out_port, int* out_tls);
int nzbget_rs_rco_webdl_check_body_chunk(
    int chunk_len, int content_len, int written_len, int gzip, int confirmed_length);

int nzbget_rs_rco_options_find_var_reference(
    const char* s, size_t* out_dollar_pos, size_t* out_var_len);
int nzbget_rs_rco_options_trim_and_check_config_line(char* line);
int nzbget_rs_rco_options_lookup_scheduler_command(const char* cmd);
int nzbget_rs_rco_options_validate_scheduler_param(
    int task_command, const char* param, int split_cmd_empty);

int nzbget_rs_rco_log_should_rotate(
    int64_t rawtime, int64_t last_written, int write_log_mode);
void nzbget_rs_rco_log_init_message_disposition(
    int target, int* out_write_to_file, int* out_keep_in_screen);
size_t nzbget_rs_rco_wstring_decode_utf8_to_utf16(
    const char* utf8, uint16_t* out, size_t out_cap);
int nzbget_rs_rco_string_builder_calc_reserve_capacity(
    int old_capacity, int requested_capacity, int exact);

int nzbget_rs_feh_frontend_calc_log_start_index(
    int needed_first_id, int first_msg_id, int* out_skip);
int nzbget_rs_feh_frontend_calc_remaining_hms(
    int64_t remaining_size, int64_t current_speed, int stand_by, int pause_download,
    int* out_h, int* out_m, int* out_s);
void nzbget_rs_feh_ncurses_calc_window_sizes(
    int nr_rows, int nr_columns, int max_screen_width, int queue_win_pct, int queue_size,
    int* out_clamped_cols, int* out_queue_win_height, int* out_queue_win_client_height,
    int* out_messages_win_top, int* out_messages_win_height,
    int* out_messages_win_client_height);
void nzbget_rs_feh_ncurses_calc_scroll_and_selection(
    int entry, int queue_size, int scroll_offset, int client_height,
    int* out_selected_entry, int* out_scroll_offset);
void nzbget_rs_feh_ncurses_calc_top_header_time(
    int time_sec, int* out_d, int* out_h, int* out_m, int* out_s);
int nzbget_rs_feh_ncurses_resolve_group_pause_action(
    int action, int64_t remaining_size, int64_t paused_size, int remaining_par_count,
    int last_pause_pars, int last_edit_entry, int selected_entry,
    int* out_last_pause_pars);
int nzbget_rs_feh_ncurses_step_rate_input(
    int key, int key_backspace, int* inout_index, int* inout_value);

int nzbget_rs_feh_maintenance_extract_package_info_str(
    const char* json, const char* key, size_t max_len,
    size_t* out_start, size_t* out_len);
int nzbget_rs_feh_maintenance_classify_update_message(const char* text);
int nzbget_rs_feh_maintenance_classify_update_info_message(const char* text);
int nzbget_rs_feh_maintenance_decode_hex_signature(
    const char* hex_sig, uint8_t* out_sig);

int nzbget_rs_snh_sys_extract_python_version(
    const char* line, size_t len, size_t* out_start, size_t* out_len);
int nzbget_rs_snh_sys_parse_unpacker_version(
    const char* line, size_t len, size_t* out_start, size_t* out_len);
int nzbget_rs_snh_sys_parse_par_version(
    const char* line, size_t len, size_t* out_pos);
void nzbget_rs_snh_os_trim_quotes(
    const char* s, size_t len, size_t* out_start, size_t* out_len);
int nzbget_rs_snh_os_parse_sw_vers_field(
    const char* output, size_t output_len, const char* prefix, size_t prefix_len,
    size_t* out_start, size_t* out_len);
const char* nzbget_rs_snh_cpu_canonical_arch(const char* arch, size_t len);
int nzbget_rs_snh_cpu_parse_cpuinfo_line(
    const char* line, size_t len, size_t* out_val_offset);

typedef struct NzbgetRsYEncoderFile NzbgetRsYEncoderFile;

NzbgetRsYEncoderFile* nzbget_rs_yencoder_open(
    const char* filename, int64_t offset, int* io_size, int64_t* out_file_size,
    const char** out_errmsg);
void nzbget_rs_yencoder_free(NzbgetRsYEncoderFile* handle);
void nzbget_rs_yencoder_write_segment(
    NzbgetRsYEncoderFile* handle, const char* filename, int part, int64_t offset,
    int size, int64_t file_size,
    void (*write_cb)(void* ctx, const char* buf, int size), void* ctx);
void nzbget_rs_nzb_generator_execute(
    const char* data_dir, int segment_size,
    void (*log_cb)(int kind, const char* msg));

typedef struct NzbgetRsNntpCache NzbgetRsNntpCache;
NzbgetRsNntpCache* nzbget_rs_nntp_cache_new(void);
void nzbget_rs_nntp_cache_free(NzbgetRsNntpCache* cache);
void nzbget_rs_nntp_cache_append(
    const NzbgetRsNntpCache* cache, const char* key, const char* data, int len);
int nzbget_rs_nntp_cache_find(
    const NzbgetRsNntpCache* cache, const char* key, const char** out_data, int* out_size);
int nzbget_rs_nntp_processor_handle_line(
    int server_id, const char* data_dir, const char* cache_dir,
    int latency, int speed, const NzbgetRsNntpCache* cache,
    const char* raw_line, void* ctx,
    void (*write_line_cb)(void* ctx, const char* line),
    void (*send_cb)(void* ctx, const char* buf, int size),
    void (*log_cb)(void* ctx, int kind, const char* msg));
int nzbget_rs_nserv_frontend_calc_start_index(
    int needed_first_id, int first_msg_id, int* out_need_skip);
void nzbget_rs_nserv_build_cmd_opts(
    const char* log_file, int verbosity, void* ctx,
    void (*push_opt_cb)(void* ctx, const char* opt));

int nzbget_rs_snh_nntp_health_cap_for(int connections, int max_cap);
int nzbget_rs_snh_nntp_health_classify_stat_reply(const char* line);
int nzbget_rs_snh_http_get_calc_watchdog_wait(
    int64_t deadline_ms, int64_t now_ms, int64_t file_size, size_t max_bytes,
    int64_t* out_wait_ms);

int nzbget_rs_snh_http_client_find_header_colon(
    const char* line, size_t len, size_t* out_colon_pos);
double nzbget_rs_snh_speed_test_calc_mbps(uint32_t bytes, double time_sec);
int nzbget_rs_snh_speed_test_is_required_more(
    size_t dataset_len, double curr_median, double prev_p25);
int nzbget_rs_snh_speed_test_exceeded_max_failures(int failures, int iterations);

int nzbget_rs_euh_data_analytics_compute(
    double* measurements, size_t len, double* out_min, double* out_max,
    double* out_avg, double* out_median, double* out_p25, double* out_p75,
    double* out_p90);
double nzbget_rs_euh_data_analytics_percentile(
    const double* sorted, size_t len, double percentile);
uint64_t nzbget_rs_euh_benchmark_limit_by_free_space(
    uint64_t requested, uint64_t available, uint64_t reserve);
size_t nzbget_rs_euh_benchmark_calc_write_chunk(
    size_t block_size, uint64_t max_bytes, uint64_t write_bytes);
const char* nzbget_rs_euh_unrar_overwrite_switch(int mode);
int nzbget_rs_euh_unrar_is_supported(const char* path, size_t len);
int nzbget_rs_euh_sevenzip_is_supported(const char* path, size_t len);
NzbgetRsBuf nzbget_rs_euh_extractor_make_password(const char* password, size_t len);
int nzbget_rs_euh_unrar_decode_exit_code(int ec, const char** out_msg);
const char* nzbget_rs_euh_sevenzip_overwrite_switch(int mode);
int nzbget_rs_euh_sevenzip_decode_exit_code(int ec, const char** out_msg);
int nzbget_rs_euh_service_should_work_now(
    int service_interval, int service_now, int64_t cur_time, int64_t last_work);
void nzbget_rs_euh_service_calc_remaining_wait(
    int service_interval, int service_now, int service_sleep, int64_t cur_time,
    int64_t last_work, int cur_wait_interval, int* out_remaining,
    int* out_wait_interval);
int nzbget_rs_euh_disk_service_interval(
    int waiting_required_dir, int disk_space_mb, int downloading, int sleep_val);
int nzbget_rs_euh_disk_service_is_low_space(int64_t available_bytes, int disk_space_mb);

int nzbget_rs_nntp_article_dl_is_retention_failure(
    int retention_days, int64_t file_time, int64_t cur_time, int* out_age_days);
int nzbget_rs_nntp_article_dl_all_servers_on_level_failed(
    int level, const int32_t* cand_ids, const int32_t* cand_levels,
    const int32_t* cand_groups, const uint8_t* cand_avail, size_t cand_count,
    const int32_t* failed_ids, const int32_t* failed_levels,
    const int32_t* failed_groups, size_t failed_count);
int nzbget_rs_nntp_article_dl_resolve_final_status(
    int status, int is_duplicate, int is_stopped, int ad_finished,
    int ad_failed, int ad_retry, int ad_fatal_error);
int nzbget_rs_nntp_article_dl_classify_decode_status(
    int raw_article, int decoder_status, int is_yenc, int range_exceeded);
void nzbget_rs_nntp_article_dl_eval_level_exhaustion(
    int all_failed, int level, int max_norm_level,
    int* out_advance_level, int* out_new_level, int* out_break_all_failed);
int nzbget_rs_nntp_article_dl_handle_content_rejected(
    int content_rejected, int dupe_fallback_round, int cur_status,
    int ad_failed, int remained_retries, int* out_should_break,
    int* out_status, int* out_content_rejected, int* out_remained_retries);
int nzbget_rs_nntp_article_dl_classify_write_failure(
    int local_write_error, int ad_fatal_error, int ad_failed,
    int* out_content_rejected);
void nzbget_rs_nntp_article_dl_normalize_connect_and_decrement_retries(
    int connected, int status, int ad_connect_error, int ad_failed, int remained_retries,
    int* out_connected, int* out_status, int* out_retries);
int nzbget_rs_nntp_article_dl_is_join_group_ok(const char* response);
int nzbget_rs_nntp_article_dl_classify_attempt_stat(
    int status, int ad_finished, int ad_failed, int ad_not_found, int ad_crc_error,
    int* out_success, int* out_failed);
int nzbget_rs_nntp_article_dl_should_advance_server_list(
    int has_want_server, int connected, int retention_failure,
    int optional_blocked, int auth_rejected, int* out_push_failed_server);
void nzbget_rs_nntp_server_pool_eval_block_server(
    int64_t prev_block_time, int64_t cur_time, int retry_interval,
    int* out_new_block, int* out_should_warn);

size_t nzbget_rs_awc_article_writer_format_result_filename(
    const char* tmp_dir, char sep, int file_id, int part_num,
    char* out_buf, size_t out_cap);
size_t nzbget_rs_awc_article_writer_format_direct_out_tmp(
    const char* dest_dir, char sep, int file_id,
    char* out_buf, size_t out_cap);
int nzbget_rs_awc_article_writer_is_within_dest_dir(
    const char* nzb_dest_dir, size_t nzb_dest_dir_len,
    const char* output_filename, size_t output_filename_len,
    char sep1, char sep2);
int nzbget_rs_awc_article_writer_classify_completion_status(
    int total_articles, int success_articles, int missed_articles, int failed_articles);
int nzbget_rs_awc_article_writer_advance_and_check_range(
    int raw_article, int* inout_ptr, int len, int article_size);
int nzbget_rs_awc_article_writer_resolve_direct_write_ready(
    int direct_write_opt, int force_direct_write, int output_initialized,
    int has_own_out, int has_force_dest, int* out_output_initialized);
int nzbget_rs_awc_article_writer_classify_setup_output_scenario(
    int raw_article, int direct_write);
int nzbget_rs_awc_article_cache_can_flush_file(
    int cached_articles, int active_downloads, int skip_disk_write, int flush_everything);
int nzbget_rs_awc_article_cache_can_alloc(
    size_t effective_limit, size_t allocated, size_t size, int* out_was_empty);
const char* nzbget_rs_awc_memory_monitor_tier_name(
    int tier, int constrained, int critical);
int nzbget_rs_awc_memory_monitor_effective_downloads_limit(
    int tier, int configured_limit, int constrained, int critical);
int nzbget_rs_awc_memory_monitor_calc_critical_or_normal_par_buffer(
    int tier, int configured_mb, int tier_critical, int tier_normal, int* out_mb);
void nzbget_rs_awc_memory_monitor_resolve_tier_limits(
    int next_tier, int raw_sample, uint32_t cfg_cache, uint32_t cfg_par,
    uint32_t cfg_limit, int tier_normal, int tier_constrained, int tier_critical,
    uint32_t rs_eff_cache, uint32_t rs_eff_par,
    uint32_t* out_eff_cache, uint32_t* out_eff_par, uint32_t* out_eff_limit);
int nzbget_rs_awc_memory_monitor_classify_tier_transition_action(
    int old_tier, int new_tier, int tier_normal, int tier_constrained,
    int tier_critical);

int nzbget_rs_ccm_conn_select_ai_family(
    int ip_version, int ip_v4, int ip_v6,
    int af_inet, int af_inet6, int af_unspec);
int nzbget_rs_ccm_conn_is_unix_domain_path(const char* host);
int nzbget_rs_ccm_conn_recv_buffered_chunk_len(int size, int buf_avail);
int nzbget_rs_ccm_conn_poll_timeout_ms(int timeout_sec);
int nzbget_rs_ccm_conn_poll_revents_ready(int revents, int mask);

int nzbget_rs_ccm_remote_client_is_http_url(const char* nzb_content);
int nzbget_rs_ccm_remote_client_calc_file_completed_pct(
    int64_t remaining_size, int64_t total_size);
void nzbget_rs_ccm_remote_client_calc_hms(
    int64_t sec, int* out_h, int* out_m, int* out_s);
int nzbget_rs_ccm_remote_client_calc_avg_speed(
    int64_t all_bytes, int64_t download_time_sec);
size_t nzbget_rs_ccm_remote_client_format_server_state(
    int download_paused, int download2_paused, int download_standby,
    uint32_t post_job_count, int post_paused, char* out_buf, size_t out_cap);

int nzbget_rs_ccm_nzbget_is_valid_umask(uint32_t umask_val);
int nzbget_rs_ccm_nzbget_should_start_secure_server(
    int secure_control, int is_unix, const char* control_ip);
int nzbget_rs_ccm_nzbget_should_stop_standalone(
    int server_mode, int queue_more, int url_more, int post_more);
int nzbget_rs_ccm_nzbget_webget_exit_code(int status, int ad_finished);
int nzbget_rs_ccm_nzbget_sigverify_exit_code(int ok);

void nzbget_rs_ccm_ncurses_calc_msg_lines(
    int text_len, int screen_width, int* out_win_width, int* out_msg_lines);
void nzbget_rs_ccm_ncurses_sanitize_message_chars(char* buf, size_t len);
const char* nzbget_rs_ccm_ncurses_select_edit_queue_keybar(
    int selected_entry, int queue_size);
int nzbget_rs_ccm_ncurses_calc_group_name_len(
    int screen_width, int calc_col_width, int col_files, int col_total, int col_left);
int nzbget_rs_ccm_ncurses_next_queue_win_pct(int cur_pct, int is_edit_queue);

int nzbget_rs_ntw_nntp_map_ip_version(
    int ip_version, int ip_v4, int ip_v6, int ip_auto);
int nzbget_rs_ntw_tls_should_verify_peer(
    int has_store, int cert_verif_level, int cv_none);
int nzbget_rs_ntw_tls_is_explicit_handshake_rejection(
    int err_lib, int err_lib_ssl, int reason, int r_handshake_fail,
    int r_proto_ver, int r_no_shared_cipher, int r_unsupported_proto);
int nzbget_rs_ntw_tls_should_check_host(
    int cert_verif_level, int cv_minimal, int host_empty);
int nzbget_rs_ntw_tls_classify_want_io(
    int ssl_err, int want_read, int want_write,
    int* out_want_read, int* out_want_write);

void nzbget_rs_ntw_webdl_calc_initial_retries(
    int url_retries, int retry_enabled, int* out_rem_dl, int* out_rem_conn);
int nzbget_rs_ntw_webdl_should_wait_before_retry(
    int status, int ad_failed, int ad_connect_error,
    int rem_dl, int rem_conn, int is_stopped, int force, int pause_download);
int nzbget_rs_ntw_webdl_is_default_http_port(const char* protocol, int port);
int nzbget_rs_ntw_webdl_is_header_end_line(int first_ch);
size_t nzbget_rs_ntw_webdl_strip_query_len(const char* url);

int nzbget_rs_ntw_server_pool_can_advance_level(
    int norm_level, int level, int active, int optional, int blocked);
int nzbget_rs_ntw_server_pool_match_want_server(
    int has_want, int cand_id, int want_id, int want_group, int cand_group);
int nzbget_rs_ntw_server_pool_match_ignore_server(
    int cand_id, int ign_id, int ign_group, int cand_group,
    int ign_norm_level, int cand_norm_level);
int nzbget_rs_ntw_server_pool_calc_remaining_block_sec(
    int64_t block_time, int retry_interval, int64_t cur_time);

int nzbget_rs_ntw_remote_server_select_port(
    int tls, int secure_port, int control_port);
int nzbget_rs_ntw_remote_server_should_warn_cap(
    int active_count, int max_processors, int64_t now, int64_t last_warn,
    int* out_should_warn);

int nzbget_rs_ntw_web_proc_max_post_content_len(int authorized);
int nzbget_rs_ntw_web_proc_is_content_len_valid(int content_len, int authorized);
int nzbget_rs_ntw_web_proc_should_use_gzip(
    int gzip_enabled, int body_len, int max_uncompressed_size);
const char* nzbget_rs_ntw_web_proc_default_resource(const char* url);

double nzbget_rs_ntw_xmlrpc_mib_per_sec(uint64_t bytes, double ms);
int nzbget_rs_ntw_xmlrpc_check_user_access_denied(
    int user_access, int ua_add, int ua_restricted, const char* method_name);
int nzbget_rs_ntw_xmlrpc_parse_dupe_mode(
    const char* dupe_mode_str, int dm_score, int dm_all, int dm_force);
int nzbget_rs_ntw_xmlrpc_is_http_url(const char* content);
int nzbget_rs_ntw_xmlrpc_parse_category_source(
    const char* cat_src_str, int cs_nzb_file, int cs_feed_file, int cs_auto);
int nzbget_rs_ntw_xmlrpc_validate_reset_counter(const char* counter);

int nzbget_rs_queue_coord_is_smaller_dupe_file(
    int64_t size1, int idx1, int64_t size2, int idx2);
int nzbget_rs_queue_coord_should_skip_health_check(
    int health_check, int hc_none, int hc_park, int par_scan, int ps_dupe,
    int health_paused, int delete_status, int ds_health, int health,
    int critical_health, int success_articles, int total_articles);
int nzbget_rs_queue_coord_is_queued_dupe_active(
    int deleting, int delete_status, int ds_none, int active_downloads,
    int has_post_info, int post_working, int post_stage, int pt_queued,
    int extra_priority, int self_completed_empty, int direct_unpack_status,
    int ns_running);

int nzbget_rs_options_normalize_time_correction(int raw_val);
int nzbget_rs_options_clamp_32bit_memory_limits(
    int is_32bit, int article_cache, int par_buffer,
    int* out_article_cache, int* out_par_buffer);

int nzbget_rs_dupesearch_classify_search_retry_reason(
    int queries, int failed, int verified_empty, int fetched_empty,
    int refused, int fetch_err, int not_tried);
size_t nzbget_rs_dupesearch_calc_fast_donor_count(
    int dupe_fast_donors, size_t live_count, int cap);
int nzbget_rs_dupesearch_alive_to_percent(double alive);
int nzbget_rs_dupesearch_fast_donor_alive_param(int is_dead, double alive);

int nzbget_rs_nntp_article_fetcher_eval_decoded_yenc(
    int check_status, int ds_finished, int format, int ef_yenc,
    int64_t begin_pos, int64_t end_pos, int64_t size, size_t data_len,
    int64_t* out_offset, int64_t* out_file_size, int* out_success);
int nzbget_rs_nntp_article_batch_can_claim_next(
    int batch_id, int cur_batch_id, int cancelled, int stopped,
    size_t next_claim, size_t requests_len, size_t next_deliver,
    int window_parts, int64_t buffered_bytes, int64_t max_buffered_bytes);

int nzbget_rs_dsp_validate_dupe_extras_header(
    int scan_cnt, int recovered_articles, int recovered_holes);
int nzbget_rs_dsp_validate_repair_job_header(
    int scan_cnt, int expected_cnt, int file_id, int par_file, int failed_articles);
int nzbget_rs_dsp_eval_restored_output_init(
    int output_initialized, int direct_write, int force_direct_write,
    int has_own_out_file, int has_out_tmp, int has_force_dest);
int nzbget_rs_dsp_calc_staged_fallback(
    int completed, int article_status, int ai_finished,
    int dupe_fallback_round, int has_result_filename,
    int ai_undefined, int* out_saved_status);
int nzbget_rs_dsp_validate_article_count(
    int size, int has_articles, int articles_len);
int nzbget_rs_dsp_need_staged_file_check(
    int completed, int format_version, int status_int, int ai_finished,
    int64_t segment_offset, int segment_size, int staged_fallback,
    int part_number, int direct_write, int force_direct_write);
int nzbget_rs_dsp_calc_volume_slot_size(
    int slot_index, int arr_size, int* out_target_size);

void nzbget_rs_queue_coord_calc_propagation_wait_log_mins(
    int64_t file_time, int propagation_delay, int64_t cur_date,
    int* out_posted_min, int* out_start_min);
int nzbget_rs_queue_coord_eval_dupe_recovery_and_cutover(
    int dupe_fallback_round, int dupe_proactive,
    const char* msg_id, const char* orig_msg_id,
    int cur_file_recovered, int cur_nzb_recovered,
    int dupe_cutover, int cutover_threshold,
    int* out_file_recovered, int* out_nzb_recovered, int* out_trigger_cutover);
int nzbget_rs_queue_coord_advance_article_completion_counters(
    int64_t rem_file_size, int64_t rem_nzb_size, int64_t paused_size,
    int article_size, int is_paused, int completed_articles, int total_articles,
    int64_t* out_rem_file_size, int64_t* out_rem_nzb_size,
    int64_t* out_paused_size, int* out_completed_articles);
int nzbget_rs_queue_coord_eval_file_completion_and_assembly(
    int file_completed, int parking, int active_downloads,
    int dupe_deleted, int file_deleted, int finished_before_cancel,
    int* out_file_completed);
void nzbget_rs_queue_coord_apply_assembly_failure_counters(
    int success_articles, int64_t success_size, int is_par_file,
    int* inout_file_failed_articles, int64_t* inout_file_failed_size,
    int* inout_nzb_cur_failed_articles, int* inout_nzb_cur_success_articles,
    int64_t* inout_nzb_cur_failed_size, int64_t* inout_nzb_cur_success_size,
    int64_t* inout_nzb_par_cur_failed_size, int64_t* inout_nzb_par_cur_success_size);
void nzbget_rs_queue_coord_demote_article_counters(
    int article_size, int is_par_file, int dupe_proactive,
    const char* msg_id, const char* orig_msg_id,
    int64_t* inout_file_success_size, int64_t* inout_file_failed_size,
    int* inout_file_success_articles, int* inout_file_failed_articles,
    int64_t* inout_nzb_cur_success_size, int64_t* inout_nzb_cur_failed_size,
    int64_t* inout_nzb_par_cur_success_size, int64_t* inout_nzb_par_cur_failed_size,
    int* inout_nzb_cur_success_articles, int* inout_nzb_cur_failed_articles,
    int* inout_file_dupe_recovered, int* inout_nzb_dupe_recovered);
int nzbget_rs_queue_coord_should_defer_dupe_failover(
    int fallback_enabled, int attempted, int recovered, int failover_sample,
    int defer_state, int dp_deferred, int par_cannot_cover,
    int failed_articles, int failover_checked, int* out_update_checked);
int nzbget_rs_queue_coord_should_park_dead_without_backup(
    int fallback_enabled, int defer_state, int dp_deferred,
    int attempted, int recovered, int tried, int projected_sample,
    int files_tried, int far_below, int below);
int nzbget_rs_queue_coord_should_wait_for_borrowing_on_par_damage(
    int fallback_enabled, int raw_article, int dupe_mode, int dm_force,
    int has_par2, int defer_state, int dp_deferred,
    int attempted, int failover_sample, int recovered);
int nzbget_rs_queue_coord_eval_discard_direct_rename_file(
    int is_par_file, int completed_articles, int active_downloads,
    int server_mode, int has_articles, int continue_partial, int cached_articles,
    int* out_discard_infos);
int nzbget_rs_queue_coord_validate_split_file_state(
    int active_downloads, int completed_articles, int has_post_info, int same_nzb);

/* Consolidated WebDownloader & StatMeter FFI (`webdownload.rs`, `statmeter.rs`) */
void nzbget_rs_webdl_step_retry_counters(
    int status, int ad_connect_error, int rem_dl, int rem_conn,
    int* out_rem_dl, int* out_rem_conn);
int nzbget_rs_webdl_resolve_body_end_status(
    int end, int status, int is_stopped,
    int ad_running, int ad_failed, int ad_finished, int* out_warn_incomplete);

void nzbget_rs_statmeter_accum_day_and_total_volume(
    int day_slot, int64_t cur_day_bytes, int cur_day_failed,
    int cur_day_success, int64_t cur_total_bytes, int64_t cur_custom_bytes,
    int64_t add_bytes, int add_failed, int add_success,
    int* out_update_day, int64_t* out_day_bytes, int* out_day_failed,
    int* out_day_success, int64_t* out_total_bytes, int64_t* out_custom_bytes);
int nzbget_rs_statmeter_calc_local_time_offset(
    int64_t utc_time, int64_t loc_time, int time_correction,
    int* out_local_delta);
int64_t nzbget_rs_statmeter_calc_resumed_download_start(
    int64_t prev_start_download, int64_t cur_time, int64_t paused_from);
int nzbget_rs_statmeter_calc_month_slots_prev_month(
    int64_t loc_cur_time, int64_t prev_month_time);
int nzbget_rs_statmeter_calc_month_slots_same_month(
    int tm_mday, int quota_start_day);

// Scheduler, Frontend, ColoredFrontend, NCursesFrontend, RemoteClient &
// WebServer Hot-Path C ABI
int nzbget_rs_sfw_scheduler_task_less(
    int hours1, int minutes1, int hours2, int minutes2);
int nzbget_rs_sfw_scheduler_should_execute_process(
    int execute_process, int task_hours, int startup_task);
int nzbget_rs_sfw_scheduler_calc_speed_limit_bytes(int kb_per_sec);
int nzbget_rs_sfw_scheduler_matches_server_ref(
    int id, int server_id, const char* server_name, const char* server_ref);
int nzbget_rs_sfw_scheduler_matches_feed_ref(
    int id, int feed_id, const char* feed_name, const char* feed_ref,
    int* out_target_id);
int nzbget_rs_sfw_scheduler_should_autoresume(
    int64_t resume_time, int64_t current_time);
uint32_t nzbget_rs_sfw_frontend_resolve_log_id_from(
    uint32_t needed_log_entries, uint32_t needed_log_first_id);
int nzbget_rs_sfw_rpc_is_response_header_valid(
    int read_ok, uint32_t resp_sig_host, uint32_t expected_sig,
    uint32_t resp_struct_size_host, uint32_t expected_struct_size);
const char* nzbget_rs_sfw_frontend_select_pause_status_suffix(
    int pause_download, int stand_by);
int nzbget_rs_sfw_frontend_calc_limit_kb(int download_limit);
int nzbget_rs_sfw_ncurses_decide_loop_update(
    int key_pressed, int* update_next_time, int data_update_pos);
int nzbget_rs_sfw_ncurses_select_sleep_interval(int is_normal_mode);
int nzbget_rs_sfw_ncurses_is_hint_active(int64_t time, int64_t start_hint);
const char* nzbget_rs_sfw_ncurses_select_normal_keybar(int group_files);
int nzbget_rs_sfw_ncurses_select_row_highlight(
    int is_edit_queue, int selected, int use_color,
    int highl_color, int text_color, char* out_brace1, char* out_brace2);
int nzbget_rs_sfw_ncurses_select_top_header_caption(
    int chars_left, int time_len, int long_cap_len, int short_cap_len);
int nzbget_rs_sfw_ncurses_calc_group_remain_hms_capped(
    int64_t unpaused_remaining_size, int64_t current_download_speed,
    int pause_download, int* out_h, int* out_m, int* out_s);
int nzbget_rs_sfw_remote_client_should_print_remaining_time(
    uint32_t download_rate, uint32_t download_paused,
    uint32_t download2_paused, uint32_t download_standby);
int nzbget_rs_sfw_web_proc_should_send_auth_response(
    int form_auth, int rpc_request, int authorized);
int nzbget_rs_sfw_web_proc_is_etag_unchanged(
    const char* old_etag, const char* new_etag);

// XmlRpc, Options, Log & NString Hot-Path C ABI
int nzbget_rs_rol_xmlrpc_validate_download_rate(
    int rate, int* out_bytes_per_sec);
int64_t nzbget_rs_rol_xmlrpc_calc_average_download_rate(
    int download_time_sec, int64_t all_bytes);
void nzbget_rs_rol_xmlrpc_nzb_message_and_post_time(
    int message_count, int cached_message_count, int post_total_sec,
    int has_post_start_time, int64_t cur_time, int64_t post_start_time,
    int* out_message_count, int* out_post_total_time_sec);
int nzbget_rs_rol_xmlrpc_post_info_log_slice(
    int log_entries, int messages_size, int* out_start, int* out_clamped_entries);
int nzbget_rs_rol_xmlrpc_save_config_check_json_keys(
    const char* key1, const char* key2);
int nzbget_rs_rol_xmlrpc_save_config_raw_baseline_valid(
    const char* raw_value);
int nzbget_rs_rol_xmlrpc_save_config_decoded_baseline_valid(
    int has_cipher_baseline, const char* decoded_value);
int nzbget_rs_rol_xmlrpc_view_feed_should_include(
    int include_non_matching, int match_status, int ms_accepted,
    int64_t size_bytes, int* out_size_mb);
int nzbget_rs_rol_xmlrpc_test_server_effective_timeout(
    int param_timeout, int article_timeout);
int nzbget_rs_rol_xmlrpc_test_server_response_ok(const char* response);
int nzbget_rs_rol_options_check_dir_join_mode(
    const char* dir, const char* parent_dir, char path_sep, char alt_path_sep);
void nzbget_rs_rol_options_normalize_dupe_and_mem_options(
    int mem_pressure_interval, int dupe_stream_timeout, int dupe_search_delay,
    int dupe_search_max_donors, int dupe_health_percent, int dupe_health_min,
    int dupe_health_max, int dupe_body_checks, int dupe_health_budget,
    int dupe_min_alive, int dupe_fast_donors,
    int* out_mem_pressure_interval, int* out_dupe_stream_timeout,
    int* out_dupe_search_delay, int* out_dupe_search_max_donors,
    int* out_dupe_health_percent, int* out_dupe_health_min,
    int* out_dupe_health_max, int* out_dupe_body_checks,
    int* out_dupe_health_budget, int* out_dupe_min_alive,
    int* out_dupe_fast_donors);
int nzbget_rs_rol_options_check_server_def(
    int has_any_field, int has_host, int has_port, int has_connections);
int nzbget_rs_rol_options_check_category_or_feed_def(
    int has_any_field, int has_required);
void nzbget_rs_rol_options_reconcile_write_flags(
    int raw_article, int skip_write, int direct_write, int direct_rename,
    int* out_direct_write, int* out_direct_rename);
int nzbget_rs_rol_log_should_close_idle(int64_t cur_time, int64_t last_written);
int nzbget_rs_rol_log_should_delete_old_rotated(
    int file_day, int cur_day, int rotate_log);
int nzbget_rs_rol_nstring_bstring_set_len(int size, int len);
int nzbget_rs_rol_nstring_bstring_append_len(
    int size, int cur_len, int str_len);
int nzbget_rs_rol_nstring_cstring_replace_bounds(
    int pos, int len, int cur_len, int raw_str_len, int str_len_param,
    int* out_add_len, int* out_del_len, int* out_new_len);
size_t nzbget_rs_rol_nstring_cstring_trim_right_len(
    const char* s, size_t len);

// Util, FileSystem, ScriptController, UnpackController & ScriptConfig Hot-Path C ABI
int64_t nzbget_rs_ufs_util_join_int64(uint32_t hi, uint32_t lo);
void nzbget_rs_ufs_util_split_int64(
    int64_t int64_val, uint32_t* out_hi, uint32_t* out_lo);
int nzbget_rs_ufs_util_match_shell_overridden_token(
    const char* shellover, const char* file_ext, size_t file_ext_len,
    size_t* out_cmd_offset);
int64_t nzbget_rs_ufs_util_internal_timegm(
    int tm_year, int tm_mon, int tm_mday, int tm_hour, int tm_min, int tm_sec);
int nzbget_rs_ufs_webutil_xml_calc_copy_len(int value_len, int value_buf_size);
int nzbget_rs_ufs_regex_calc_match_len(int rm_so, int rm_eo);
int nzbget_rs_ufs_tokenizer_fits_short_string(int len, int capacity);
int nzbget_rs_ufs_fs_validate_load_file_size(
    int64_t file_size, int add_trailing_null, int* out_size, int* out_buf_size);
int64_t nzbget_rs_ufs_fs_calc_allocate_chunk_size(
    int64_t remaining, int64_t zeros_size);
int nzbget_rs_ufs_fs_classify_expand_file_name(
    const char* filename, size_t* out_skip_dot_slash);
int nzbget_rs_ufs_dir_browser_is_dot_or_dotdot(const char* filename);
int nzbget_rs_ufs_disk_file_select_seek_origin(
    int origin, int so_cur, int so_end, int seek_set, int seek_cur, int seek_end);
int nzbget_rs_ufs_script_ctrl_matches_env_prefix(
    const char* opt_name, const char* strip_prefix, int prefix_len);
int nzbget_rs_ufs_script_ctrl_is_fork_start_error_line(
    int first_line, const char* buf);
int nzbget_rs_ufs_script_ctrl_normalize_fork_exit_code(
    int exit_code, int fork_error_exit_code, int start_error);
int nzbget_rs_ufs_script_ctrl_calc_signal_exit_code(int term_sig);
int nzbget_rs_ufs_script_ctrl_select_kill_pid(int child_pid, int child_pgid);

// DownloadInfo & DiskState C ABI (rust/src/downloadinfo.rs, rust/src/diskstatepp.rs)
typedef struct NzbgetRsDpsCurrentStatsAcc {
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
} NzbgetRsDpsCurrentStatsAcc;

int nzbget_rs_dsp_inspect_read_line(
    const char* buf, int* out_needs_scroll, size_t* out_new_len);
int nzbget_rs_dsp_check_queue_format_version(
    int format_version, int queue_version,
    int* out_is_legacy, int* out_load_inline_history);
int nzbget_rs_dsp_parse_legacy_file_state_trailer(
    const char* line, int64_t* out_decoded_size, int* out_output_initialized);
int nzbget_rs_dsp_feed_status_matches(
    const char* feed_url, const char* url, int format_version,
    const char* feed_filter, const char* filter,
    uint32_t feed_filter_hash, uint32_t filter_hash);

int nzbget_rs_queue_coord_select_dupe_donor_id(
    int dupe_fallback_round, const int* donors_ptr, size_t donors_len,
    const char* msg_id, const char* orig_msg_id);
void nzbget_rs_queue_coord_apply_failed_article_counters(
    int part_number, int par_file, int article_size, int dupe_failed_run,
    int64_t file_failed_size, int64_t nzb_cur_failed_size,
    int64_t nzb_par_cur_failed_size, int file_failed_articles,
    int nzb_cur_failed_articles, int* out_dupe_failed_run,
    int64_t* out_file_failed_size, int64_t* out_nzb_cur_failed_size,
    int64_t* out_nzb_par_cur_failed_size, int* out_file_failed_articles,
    int* out_nzb_cur_failed_articles);
void nzbget_rs_queue_coord_eval_delete_file_diskstate_flags(
    int file_status, int cf_success, int cf_partial, int file_deleted,
    int parking, int completed, int* out_delete_data, int* out_save_partial);
void nzbget_rs_queue_coord_discard_downloaded_articles_counters(
    int64_t nzb_rem_size, int64_t nzb_paused_size, int nzb_cur_succ_art,
    int64_t nzb_cur_succ_size, int64_t nzb_par_cur_succ_size,
    int nzb_cur_fail_art, int64_t nzb_cur_fail_size,
    int64_t nzb_par_cur_fail_size, int file_paused, int file_succ_art,
    int64_t file_succ_size, int file_fail_art, int64_t file_fail_size,
    int64_t file_size, int64_t file_missed_size,
    int64_t* out_nzb_rem_size, int64_t* out_nzb_paused_size,
    int* out_nzb_cur_succ_art, int64_t* out_nzb_cur_succ_size,
    int64_t* out_nzb_par_cur_succ_size, int* out_nzb_cur_fail_art,
    int64_t* out_nzb_cur_fail_size, int64_t* out_nzb_par_cur_fail_size,
    int64_t* out_file_rem_size);

// XmlRpc, Options, NZBGet & RemoteClient Hot-Path C ABI
int nzbget_rs_roc_xmlrpc_multicall_needs_array_wrap(const char* resp);
int nzbget_rs_roc_xmlrpc_tls_cipher_access_allowed(
    int user_access, int ua_control, int protocol, int rp_json_rpc,
    int http_method, int hm_post);
int nzbget_rs_roc_xmlrpc_tls_cipher_validate_job_id(
    int operation, size_t param_count, int first_is_string,
    const char* id_ptr, size_t id_len);
void nzbget_rs_roc_xmlrpc_status_calc_mbytes(
    int64_t remaining_size, int64_t forced_size, int64_t article_cache,
    int64_t all_bytes, int64_t month_bytes, int64_t day_bytes,
    int* out_remaining_mb, int* out_forced_mb, int* out_cache_mb,
    int* out_downloaded_mb, int* out_month_mb, int* out_day_mb);
void nzbget_rs_roc_xmlrpc_status_calc_disk_space_mb(
    int64_t free_disk, int64_t total_disk, int inter_dir_empty,
    int64_t free_inter, int64_t total_inter,
    int64_t* out_free_inter, int64_t* out_total_inter,
    int* out_free_disk_mb, int* out_total_disk_mb,
    int* out_free_inter_mb, int* out_total_inter_mb);
int nzbget_rs_roc_xmlrpc_log_params_valid(int id_from, int nr_entries);
int nzbget_rs_roc_xmlrpc_list_files_params_valid(
    int has_id_start, int has_id_end, int id_start, int id_end, int nzb_id);
void nzbget_rs_roc_xmlrpc_post_info_elapsed_sec(
    int64_t cur_time, int64_t stage_time, int64_t start_time,
    int* out_stage_sec, int* out_total_sec);
void nzbget_rs_roc_xmlrpc_list_groups_calc_mb(
    int64_t remaining_size, int64_t paused_size,
    int* out_remaining_mb, int* out_paused_mb);
int nzbget_rs_roc_xmlrpc_history_should_skip_dup(
    int kind, int hk_dup, int include_dup);
int nzbget_rs_roc_xmlrpc_config_should_mask_value(
    int user_access, int ua_restricted, int opt_restricted);
void nzbget_rs_roc_xmlrpc_server_volumes_calc_mb(
    int64_t total_bytes, int64_t custom_bytes,
    uint32_t* out_total_mb, uint32_t* out_custom_mb);
int nzbget_rs_roc_xmlrpc_test_server_speed_check_server(
    int has_server, int is_active, int max_connections);
int nzbget_rs_roc_xmlrpc_test_disk_speed_validate_params(
    int write_buffer_kib, int max_file_size_gib, int timeout_sec,
    size_t* out_buffer_bytes, uint64_t* out_max_file_bytes);
int nzbget_rs_roc_options_server_cipher_without_tls(
    int tls_enabled, const char* cipher);
void nzbget_rs_roc_options_reconcile_conflict_rules(
    int dupe_search, int health_check, int hc_park,
    int flush_queue, int direct_write,
    int unpack_pause_queue, int direct_unpack,
    int* out_disable_dupe_search, int* out_enable_flush_queue,
    int* out_enforce_unpack_mutex);
int nzbget_rs_roc_nzbget_boot_is_direct_client_mode(
    int test_backtrace, int web_get, int sig_verify,
    int client_op, int op_no_operation, int print_options);
int nzbget_rs_roc_nzbget_boot_should_pause_on_config_errors(
    int config_errors, int client_op, int op_no_operation);
int64_t nzbget_rs_roc_nzbget_reload_clamp_debounce_ms(
    int64_t elapsed_since_first_ms, int64_t debounce_delay_ms,
    int64_t max_burst_delay_ms);
int64_t nzbget_rs_roc_remote_client_calc_group_unpaused_remaining(
    int64_t remaining_size, int64_t paused_size, int active_downloads,
    int* out_has_threads, int* out_threads_plural);

void nzbget_rs_binrpc_log_slice_bounds(
    int lines, uint32_t id_from, size_t messages_len, uint32_t first_msg_id,
    int* out_start, int* out_nr_entries);

int nzbget_rs_dupesearch_slot_counts_as_used(
    int is_member, int is_valid, int score, int dead_score,
    int id, int dry_run, int placement_available);
int nzbget_rs_dupesearch_is_usable_whole_backup(
    int is_valid, int score, int dead_score, double alive,
    int id, int dry_run, int placement_available);
int nzbget_rs_dupesearch_eval_post_place_retry(
    int rank_only, int state_retryable, int usable_backup,
    size_t unchecked_members_len, int* out_retry_discovery,
    int* out_rank_incomplete);
int nzbget_rs_dupesearch_needs_nzb_ext(
    const char* name_ptr, size_t name_len);
void nzbget_rs_dupesearch_classify_pending_phase(
    const char* phase_ptr, size_t phase_len,
    int* out_fetched, int* out_ranking);

void nzbget_rs_queue_coord_apply_success_article_counters(
    int64_t article_size, int is_par_file, int64_t file_success_size,
    int64_t nzb_cur_success_size, int64_t nzb_par_cur_success_size,
    int file_success_articles, int nzb_cur_success_articles,
    int64_t* out_file_success_size, int64_t* out_nzb_cur_success_size,
    int64_t* out_nzb_par_cur_success_size, int* out_file_success_articles,
    int* out_nzb_cur_success_articles);

int nzbget_rs_dqh_fleet_clamp_timeout_sec(
    int timeout_sec, int max_timeout_sec);
void nzbget_rs_dqh_fleet_update_key_score_bounds(
    int64_t cur_top, int cur_lowest, int any_seen, int queued,
    int deleting, int delete_status, int ds_none_val,
    int cur_running_id, int cur_running_score, int item_id, int item_score,
    int64_t* out_top, int* out_lowest, int* out_running_id,
    int* out_running_score);
int nzbget_rs_dqh_fleet_is_already_downloaded_history_item(
    int has_item, int delete_status, int ds_none_val, int is_dupe_success,
    int mark_status, int ks_bad_val);
int nzbget_rs_dqh_fleet_calc_member_add_score(
    int running_id, int lowest, int64_t top, int rank);
int nzbget_rs_dqh_donor_health_error_increment(int busy);
int nzbget_rs_dqh_donor_health_should_pause_server(
    int errors, int server_give_up);
void nzbget_rs_dqh_donor_health_accum_verdict_counts(
    int verdict, int f_present, int f_missing, int f_error, int soft,
    int* io_present, int* io_missing, int* io_error, int* io_body_bad);

int nzbget_rs_dupesearch_is_rank_only_queued_member(
    int rank_only, int is_self, int kind, int nk_nzb_val, int dupe_mode,
    int dm_score_val, int has_post_info, int delete_status, int ds_none_val,
    int ds_dupe_val, int item_score, int pick_score);
int nzbget_rs_dupesearch_calc_expand_budget_ms(
    int budget_sec, int64_t deadline, int64_t now);
int nzbget_rs_dupe_identity_decode_inner_buf(
    const char* raw_data, size_t raw_len, uint64_t* out_bytes,
    size_t* out_ext_len, size_t* out_name_offset, size_t* out_name_len);
uint64_t nzbget_rs_dupe_identity_calc_min_inner_bytes(
    int64_t data_bytes);
int nzbget_rs_nntp_health_classify_body_fetch_result(
    int success, int transient, int ans_present, int ans_body_bad,
    int* out_throw_error);
void nzbget_rs_dupesearch_fnv1a_fingerprint(
    const char* const* ids, const size_t* id_lens, size_t id_count,
    char* out_hex16);

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

int nzbget_rs_xmlrpc_match_command(const char* method_name);
NzbgetRsBuf nzbget_rs_xmlrpc_format_error(int is_json, int err_code, const char* err_text);
NzbgetRsBuf nzbget_rs_xmlrpc_format_bool(int is_json, int ok);
NzbgetRsBuf nzbget_rs_xmlrpc_format_int(int is_json, int value);
NzbgetRsBuf nzbget_rs_xmlrpc_format_string(int is_json, const char* raw_str, int encode, size_t max_len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_status(
    int is_json, const NzbgetRsRpcStatusInput* inp,
    const NzbgetRsRpcServerStatus* servers, size_t servers_len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_log_list(
    int is_json, const NzbgetRsRpcLogEntry* entries, size_t len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_file_item(
    int is_json, int index, const NzbgetRsRpcFileItem* item);
NzbgetRsBuf nzbget_rs_xmlrpc_format_nzb_info_fields(
    int is_json, const NzbgetRsRpcNzbInfoInput* info,
    const NzbgetRsRpcParamItem* params, size_t params_len,
    const NzbgetRsRpcScriptItem* scripts, size_t scripts_len,
    const NzbgetRsRpcServerStatItem* stats, size_t stats_len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_post_info_fields(
    int is_json, int post_queue, int has_post_info, const char* progress_label,
    int stage_progress, int64_t cur_time, int64_t stage_time, int64_t start_time,
    const NzbgetRsRpcLogEntry* logs, size_t logs_len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_group_item_start(
    int is_json, int index, int nzb_id, int64_t remaining_size, int64_t paused_size,
    int remaining_file_count, int remaining_par_count, int priority,
    int active_downloads, const char* status);
NzbgetRsBuf nzbget_rs_xmlrpc_format_postqueue_item_start(
    int is_json, int index, int nzb_id, const char* info_name, int stage, int file_progress);
NzbgetRsBuf nzbget_rs_xmlrpc_format_history_nzb_start(
    int is_json, int index, int id, const char* name, int remaining_file_count,
    int retry_data, int history_time, const char* status);
NzbgetRsBuf nzbget_rs_xmlrpc_format_history_dup_item(
    int is_json, int index, int id, const char* name, int history_time, int64_t size,
    const char* dupe_key, int dupe_score, int dupe_mode, int dup_status, const char* status);
NzbgetRsBuf nzbget_rs_xmlrpc_format_urlqueue_item(
    int is_json, int index, int id, const char* filename, const char* url,
    const char* name, const char* category, int priority);
NzbgetRsBuf nzbget_rs_xmlrpc_format_config_list(
    int is_json, int user_access, int ua_restricted,
    const NzbgetRsRpcConfigItem* entries, size_t len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_config_templates(
    int is_json, const char* const* templates, size_t len);
NzbgetRsBuf nzbget_rs_xmlrpc_format_feed_item(
    int is_json, int index, const NzbgetRsRpcFeedItem* item);
NzbgetRsBuf nzbget_rs_xmlrpc_format_server_volume_item(
    int is_json, int index, const NzbgetRsRpcServerVolumeItem* item);
NzbgetRsBuf nzbget_rs_xmlrpc_format_fleet_result(
    int chosen, int complete, const char* reason,
    const NzbgetRsRpcFleetMember* members, size_t members_len);

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

NzbgetRsBuf nzbget_rs_diskstate_format_nzb_info(
    const NzbgetRsDiskNzbInfoInput* inp,
    const NzbgetRsDiskCompletedFile* completed, size_t completed_len,
    const NzbgetRsDiskParam* params, size_t params_len,
    const NzbgetRsDiskScriptStatus* scripts, size_t scripts_len,
    const NzbgetRsDiskServerStat* stats, size_t stats_len,
    const NzbgetRsDiskFileStub* files, size_t files_len);
NzbgetRsBuf nzbget_rs_diskstate_format_dupe_extras(
    int recovered_articles, int64_t recovered_bytes, int recovered_holes,
    const NzbgetRsDiskRepairJob* jobs, size_t jobs_len);
NzbgetRsBuf nzbget_rs_diskstate_format_file_info(
    const NzbgetRsDiskFileInfoInput* inp,
    const char* const* groups, size_t groups_len,
    int include_articles,
    const NzbgetRsDiskArticleInfo* articles, size_t articles_len);
NzbgetRsBuf nzbget_rs_diskstate_format_file_state(
    const NzbgetRsDiskFileStateInput* inp,
    const NzbgetRsDiskServerStat* stats, size_t stats_len,
    const NzbgetRsDiskArticleState* articles, size_t articles_len);
NzbgetRsBuf nzbget_rs_diskstate_format_dup_info(
    int status, int64_t size, uint32_t full_content_hash, uint32_t filtered_content_hash,
    int dupe_score, int dupe_mode, const char* name, const char* dupe_key);
NzbgetRsBuf nzbget_rs_diskstate_format_volume_stat_item(
    const NzbgetRsDiskVolumeStatInput* inp);

typedef struct {
    int id;
    const char* nzb_name;
    const char* filename;
    int64_t size;
    int64_t remaining_size;
    int active_downloads;
    int paused;
    int matched;
} NzbgetRsBinRpcFileListingEntry;

typedef struct {
    const char* name;
    const char* value;
} NzbgetRsBinRpcGroupParamEntry;

typedef struct {
    int id;
    int kind;
    int priority;
    const char* name;
    const char* category;
    int file_count;
    int64_t remaining_size;
    int64_t paused_size;
    int active_downloads;
    int matched;
    const NzbgetRsBinRpcGroupParamEntry* params;
    size_t param_count;
} NzbgetRsBinRpcGroupListingEntry;

typedef struct {
    int include_remaining_size;
    int64_t remaining_size;
    uint32_t download_rate;
    uint32_t download_paused;
    uint32_t download2_paused;
    uint32_t download_stand_by;
    int64_t downloaded_bytes;
    uint32_t download_time_sec;
    uint32_t download_limit;
    uint32_t up_time_sec;
    uint32_t thread_count;
    uint32_t post_job_count;
    uint32_t scan_paused;
    uint32_t post_paused;
} NzbgetRsBinRpcStatusSummarySpec;

typedef struct {
    int stand_by;
    int pause_download;
    int64_t current_download_speed;
    int64_t remaining_size;
    int64_t download_limit;
    int post_job_count;
    int dn_time_sec;
    int64_t all_bytes;
    int thread_count;
} NzbgetRsNcursesStatusSpec;

typedef struct {
    int edit_queue_mode;
    int selected;
    int use_color;
    int color_highl;
    int color_text;
    int id;
    int priority;
    int active_downloads;
    int paused;
    int show_nzbname;
    uint8_t path_sep;
    const char* nzb_name;
    const char* filename;
    int64_t size;
    int64_t remaining_size;
} NzbgetRsNcursesFileRowSpec;

typedef struct {
    int edit_queue_mode;
    int selected;
    int use_color;
    int color_highl;
    int color_text;
    int screen_width;
    int calc_col_width;
    int id;
    int priority;
    int active_downloads;
    const char* name;
    int file_count;
    int paused_file_count;
    int64_t size;
    int64_t remaining_size;
    int64_t paused_size;
    int stand_by;
    int pause_download;
    int64_t current_download_speed;
} NzbgetRsNcursesGroupRowSpec;

NzbgetRsBuf nzbget_rs_cmdline_format_usage(
    const char* base_name, char path_sep, int include_daemon);
NzbgetRsBuf nzbget_rs_binrpc_file_listing(
    const NzbgetRsBinRpcFileListingEntry* entries, size_t len, int has_pattern);
NzbgetRsBuf nzbget_rs_binrpc_group_listing(
    const NzbgetRsBinRpcGroupListingEntry* entries, size_t len, int has_pattern);
NzbgetRsBuf nzbget_rs_binrpc_status_summary(
    const NzbgetRsBinRpcStatusSummarySpec* spec);
NzbgetRsBuf nzbget_rs_ncurses_format_status_bar(
    const NzbgetRsNcursesStatusSpec* spec);
NzbgetRsBuf nzbget_rs_ncurses_format_top_header(
    const char* header, int screen_width, int up_time, int time_sec);
NzbgetRsBuf nzbget_rs_ncurses_format_file_queue_header(
    int use_color, int file_num, int paused_files, int64_t remaining, int64_t paused);
NzbgetRsBuf nzbget_rs_ncurses_format_group_queue_header(
    int use_color, int group_count, int64_t remaining, int64_t paused);
NzbgetRsBuf nzbget_rs_ncurses_format_file_row(
    const NzbgetRsNcursesFileRowSpec* spec, int* out_color);
NzbgetRsBuf nzbget_rs_ncurses_format_group_row(
    const NzbgetRsNcursesGroupRowSpec* spec,
    int* inout_col_files, int* inout_col_total, int* inout_col_left, int* out_color);

NzbgetRsBuf nzbget_rs_colored_frontend_format_status_bar(
    int stand_by, int pause_download, int64_t current_download_speed,
    int64_t remaining_size, int64_t download_limit, int post_job_count,
    int thread_count, const char* control_seq);
NzbgetRsBuf nzbget_rs_frontend_format_log_message(
    int colored, int kind, const char* text);

NzbgetRsBuf nzbget_rs_web_format_auth_response(
    int form_auth, int keep_alive, const char* version);
NzbgetRsBuf nzbget_rs_web_format_options_response(
    int keep_alive, const char* origin, const char* version);
NzbgetRsBuf nzbget_rs_web_format_error_response(
    const char* err_code, int keep_alive, const char* version, NzbgetRsBuf* out_body);
NzbgetRsBuf nzbget_rs_web_format_redirect_response(
    const char* url, int keep_alive, const char* version);
NzbgetRsBuf nzbget_rs_web_format_body_response_header(
    int unchanged, int keep_alive, const char* origin, int form_auth,
    const char* auth_token, int body_len, const char* content_type,
    int gzip, const char* etag, const char* version);

NzbgetRsBuf nzbget_rs_diskstate_format_unpack_trailer_item(
    int id, int direct_unpack_status, int unpack_tried, int par_repaired,
    int last_unpack_status, const char* const* archives, size_t archives_len);
NzbgetRsBuf nzbget_rs_diskstate_format_feed_status(
    const char* const* urls, const uint32_t* filter_hashes,
    const int* last_updates, size_t len);
NzbgetRsBuf nzbget_rs_diskstate_format_feed_history(
    const int* statuses, const int* last_seens,
    const char* const* urls, size_t len);
NzbgetRsBuf nzbget_rs_diskstate_format_server_info(
    const char* const* names, const char* const* hosts,
    const int* ports, const char* const* users, size_t len);
NzbgetRsBuf nzbget_rs_diskstate_format_nzb_message_line(
    const char* time_str, int tm, int kind, const char* text, const char* line_ending);
int nzbget_rs_diskstate_parse_completed_file_line(
    char* buf, int format_version, int* out_id, int* out_status,
    uint32_t* out_crc, int* out_par_file, const char** out_hash16k,
    const char** out_par_set_id, const char** out_file_name);

NzbgetRsBuf nzbget_rs_xmlrpc_format_disk_speed(
    int is_json, uint64_t write_bytes, double write_ms,
    uint64_t read_bytes, double read_ms);
NzbgetRsBuf nzbget_rs_xmlrpc_format_network_speed(
    int is_json, double speed_mbps);
int nzbget_rs_rco_rpc_save_config_find_cipher_conflict(
    const char* const* names, const char* const* values, size_t len);

int nzbget_rs_options_parse_enum_value(
    const char* val, const char* def_val, int argc,
    const char* const* argn, const int* argv,
    int* out_value, int* out_norm_idx);
int nzbget_rs_options_parse_int_value(
    const char* val, const char* def_val, int base, int* out_value);

uint32_t nzbget_rs_dupe_watch_build_item_flags(
    int kind, int in_history, int dupe_success, int deleting,
    int delete_status, int has_post_info, int dupe_mode,
    int is_donor, int dupe_hint, int has_dupe_fleet);
int nzbget_rs_dupe_watch_is_live_fleet_peer(
    const char* peer_key, size_t peer_key_len,
    const char* pick_key, size_t pick_key_len, uint32_t peer_flags);
NzbgetRsBuf nzbget_rs_dupe_watch_format_searched_state_line(
    int64_t now, int64_t state_ttl_sec, const char* key, int score,
    int64_t searched_time, const char* title, int retryable);

typedef struct {
    int file_count;
    int active_downloads;
    uint32_t full_content_hash;
    uint32_t filtered_content_hash;
    int64_t size;
    int64_t remaining_size;
    int paused_file_count;
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
    int remaining_par_count;
    int total_articles;
    int success_articles;
    int failed_articles;
    int current_success_articles;
    int current_failed_articles;
    int64_t min_time;
    int64_t max_time;
    int64_t downloaded_size;
    int download_sec;
    int64_t download_start_time;
} NzbgetRsNzbMergeSplitCounters;

typedef struct {
    int64_t size;
    int64_t remaining_size;
    int64_t success_size;
    int64_t failed_size;
    int64_t missed_size;
    int total_articles;
    int missed_articles;
    int success_articles;
    int failed_articles;
    int par_file;
    int paused;
} NzbgetRsSplitFileCounters;

void nzbget_rs_queue_coord_merge_counters(
    NzbgetRsNzbMergeSplitCounters* dest, const NzbgetRsNzbMergeSplitCounters* src);
void nzbget_rs_queue_coord_split_transfer_file(
    NzbgetRsNzbMergeSplitCounters* src, NzbgetRsNzbMergeSplitCounters* dst,
    const NzbgetRsSplitFileCounters* file);

NzbgetRsBuf nzbget_rs_nserv_format_usage(const char* base_name);

void nzbget_rs_binrpc_init_message_base(
    uint8_t* out, int request_type, int struct_size,
    const char* username, const char* password);
void nzbget_rs_binrpc_init_download_request(
    uint8_t* out, const char* username, const char* password,
    const char* nzb_filename, const char* nzb_content, int is_url,
    const char* category, int add_first, int add_paused, int priority,
    int dupe_score, int dupe_mode, const char* dupe_key,
    int trailing_len, int auto_category);
NzbgetRsBuf nzbget_rs_binrpc_encode_edit_queue_trailing(
    const char* text, const int32_t* ids, size_t ids_len,
    const char* const* names, size_t names_len,
    int* out_text_len, int* out_names_len);
NzbgetRsBuf nzbget_rs_binrpc_file_listing_from_wire(
    const char* data, size_t len, uint32_t nr_nzb, uint32_t nr_ppp,
    uint32_t nr_files, int has_pattern);
NzbgetRsBuf nzbget_rs_binrpc_group_listing_from_wire(
    const char* data, size_t len, uint32_t nr_nzb, uint32_t nr_ppp,
    uint32_t nr_files, int has_pattern);

int nzbget_rs_stream_select_whole_file_donor(
    const char* target_filename, int64_t target_encoded_size, int target_article_count,
    uint64_t target_steps_hash, const char* donor_set,
    const char* const* donor_filenames, const int64_t* donor_sizes,
    const uint8_t* donor_is_par, const uint8_t* donor_has_articles,
    const uint8_t* donor_is_claimed, const uint64_t* donor_steps_hashes,
    size_t donor_count);
size_t nzbget_rs_stream_select_donor_candidates(
    const char* target_filename, int64_t target_decoded_file_size,
    int positional_rank, int positional_window, int max_candidates,
    uint64_t target_steps_hash,
    const char* const* donor_filenames, const int64_t* donor_sizes,
    const uint8_t* donor_is_par, const uint8_t* donor_has_articles,
    const uint8_t* donor_is_claimed, const uint64_t* donor_steps_hashes,
    size_t donor_count, int32_t* out_indices, size_t out_cap);
size_t nzbget_rs_stream_select_intact_proof_candidates(
    const char* intact_basename, int64_t intact_size, const char* donor_set,
    int stand_in_index, int max_candidates,
    const char* const* donor_filenames, const int64_t* donor_sizes,
    const uint8_t* donor_has_articles, const uint8_t* donor_is_claimed,
    size_t donor_count, int32_t* out_indices, size_t out_cap);
void nzbget_rs_stream_select_verify_donor_probes(
    const NzbgetRsByteRange* donor_ranges, size_t donor_count,
    const NzbgetRsByteRange* holes, size_t hole_count,
    int64_t decoded_file_size, int probe_count,
    int32_t* out_probes, size_t probes_cap, size_t* out_probes_len,
    int32_t* out_reserve, size_t reserve_cap, size_t* out_reserve_len);
int64_t nzbget_rs_stream_encrypted_compare_floor(
    int64_t inner_size, int64_t block_size,
    const NzbgetRsContentRun* runs, size_t run_count,
    const NzbgetRsByteRange* original_holes, size_t original_hole_count,
    const NzbgetRsByteRange* windows, size_t window_count);
void nzbget_rs_stream_partition_encrypted_chunk(
    int64_t block_from, int64_t chunk_end,
    const NzbgetRsByteRange* live_holes, size_t live_hole_count,
    NzbgetRsByteRange* out_hole_parts, size_t hole_parts_cap, size_t* out_hole_parts_len,
    NzbgetRsByteRange* out_present_parts, size_t present_parts_cap, size_t* out_present_parts_len);
void nzbget_rs_content_mapper_collect_inner_holes(
    const NzbgetRsContentRun* runs, size_t run_count,
    const int32_t* hole_member_indices, const NzbgetRsByteRange* holes, size_t hole_count,
    void* ctx,
    void (*on_holes_cb)(void* ctx, const NzbgetRsByteRange* inner_holes, size_t len));

typedef struct NzbgetRsRarFileEntry {
    int32_t volume_index;
    const char* filename;
    int64_t size;
    int64_t packed_size;
    int64_t data_offset;
    uint8_t stored;
    uint8_t encrypted_data;
    uint8_t has_salt;
    uint8_t salt[8];
    uint8_t has_crypt;
    int32_t crypt_version;
    int32_t crypt_kdf_count;
    uint8_t crypt_salt[16];
    uint8_t crypt_iv[16];
} NzbgetRsRarFileEntry;

void nzbget_rs_content_mapper_build_rar_map(
    int unknown_volume, int has_password,
    const uint8_t* vol_readable, const int32_t* vol_versions,
    const int32_t* vol_member_indices, const uint8_t* vol_has_source,
    const int64_t* vol_source_sizes, size_t vol_count,
    const NzbgetRsRarFileEntry* entries, size_t entry_count,
    void* ctx,
    int (*make_crypto_cb)(void* ctx, size_t first_vol, int version),
    void (*on_map_cb)(
        void* ctx, const char* inner_name, int64_t inner_size,
        const NzbgetRsContentRun* runs, size_t run_count,
        int encrypted, const int64_t* cipher_data_offsets, const int64_t* cipher_sizes),
    void (*on_skip_cb)(void* ctx, const char* reason));
NzbgetRsBuf nzbget_rs_stream_select_extracted_inner(
    const char* dir, int64_t inner_size, const char* inner_name, int* out_found_symlink);
void nzbget_rs_dupe_fallback_list_par2_files(
    const char* dir, void* ctx,
    void (*on_entry)(void* ctx, const char* name, size_t name_len, uint64_t length, uint64_t block_size));
size_t nzbget_rs_dupe_fallback_borrowed_par2_mismatches(
    const char* path, const char* dest_dir, const char* filename,
    const int32_t* statuses, const int32_t* donor_ids, const int32_t* fallback_rounds,
    const int64_t* segment_offsets, const int32_t* segment_sizes, size_t article_count,
    uint32_t (*crc_cb)(const uint8_t* data, size_t len),
    int32_t* out_indices, size_t out_cap);

int32_t nzbget_rs_donorscore_take_rank(
    double alive, int32_t twin, const int32_t* used_ptr, size_t used_len);
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
NzbgetRsBuf nzbget_rs_twin_format_kind_message(
    const char* nzb_name, const char* primary_name, const char* kind, int32_t by_par2);
size_t nzbget_rs_queue_coord_find_dupe_file_indices(
    const char* const* filenames, const int64_t* sizes, size_t count,
    size_t* out_dupe_indices, size_t out_cap);
NzbgetRsBuf nzbget_rs_ufs_util_resolve_path_from_env(
    const char* path_ptr, size_t path_len, const char* env_path);

int32_t nzbget_rs_ext_script_kind(const char* line_ptr, size_t line_len);
void nzbget_rs_feh_ext_sort_indices(
    const char* const* names, size_t count, const char* order_str, size_t* out_indices);
int32_t nzbget_rs_dupesearch_parse_cooldown_line(
    const char* line_ptr, size_t line_len, int64_t now_sec,
    NzbgetRsBuf* out_indexer, int64_t* out_when);
NzbgetRsBuf nzbget_rs_dupesearch_format_cooldown_line(
    const char* indexer_ptr, size_t indexer_len, int64_t when_sec, int64_t now_sec);
NzbgetRsBuf nzbget_rs_dqh_fleet_ensure_nzb_ext(const char* name_ptr, size_t name_len);
void nzbget_rs_dqh_fleet_visit_pipe_names(
    const char* names_ptr, size_t names_len, void* ctx,
    void (*visit)(void* ctx, const char* part_ptr, size_t part_len));

int32_t nzbget_rs_snh_http_client_validate_version(
    const char* version_ptr, size_t version_len, int32_t stream_ok);
NzbgetRsBuf nzbget_rs_snh_http_client_build_headers(
    const char* method_ptr, size_t method_len,
    const char* host_ptr, size_t host_len,
    const char* version_cstr, int32_t include_gzip);
double nzbget_rs_snh_speed_test_final_speed(
    double first_median, const double* p90s_ptr, size_t p90s_len);
NzbgetRsBuf nzbget_rs_snh_speed_test_make_request(
    const char* path_ptr, size_t path_len,
    const char* query_ptr, size_t query_len,
    uint32_t filesize,
    const char* host_ptr, size_t host_len);
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
int32_t nzbget_rs_rol_xmlrpc_multicall_find_struct_end(
    const char* request_ptr, size_t* out_struct_off);
int32_t nzbget_rs_rol_xmlrpc_multicall_locate_parts(
    const char* request_ptr, size_t request_len, size_t* out_name_off, size_t* out_params_off);
int32_t nzbget_rs_twin_compare_par2_data(
    const char* const* own_names_ptr,
    const char* const* own_md5s_ptr,
    size_t own_count,
    const char* own_print_ptr,
    const uint8_t* twin_data_ptr,
    size_t twin_data_len);
int64_t nzbget_rs_stream_composite_compute_member_bases(
    const int64_t* member_sizes_ptr, int64_t* out_bases_ptr, size_t count);
int64_t nzbget_rs_stream_content_map_sum_cipher_sizes(
    const int64_t* cipher_sizes_ptr, size_t count);
NzbgetRsBuf nzbget_rs_stream_content_mapper_split_inner_name(
    const char* first_name);
size_t nzbget_rs_stream_order_donor_indices(
    const int32_t* dupe_scores_ptr, const int32_t* ids_ptr,
    size_t count, size_t* out_indices_ptr);
int32_t nzbget_rs_stream_is_intact_proof_member_eligible(
    const char* base_name, const char* member_name, const char* target_set,
    const char* const* target_filenames_ptr, size_t target_count);
void nzbget_rs_dqh_donor_health_accum_health(
    int32_t* checked, int32_t* present, int32_t* missing, int32_t* error,
    int32_t* body_checked, int32_t* body_bad,
    int32_t add_checked, int32_t add_present, int32_t add_missing,
    int32_t add_error, int32_t add_body_checked, int32_t add_body_bad);
int32_t nzbget_rs_dqh_donor_health_calc_worker_budget_ms(
    int64_t deadline_ms, int64_t now_ms, int32_t cur_budget_ms,
    int32_t* out_budget_ms);
size_t nzbget_rs_dqh_donor_health_calc_thread_count(
    size_t postings_len, int32_t concurrency);
NzbgetRsBuf nzbget_rs_dqh_fleet_format_fetch_error(int32_t status);
void nzbget_rs_dqh_fleet_calc_check_budget_and_deadline(
    int32_t opt_max, int32_t fleet_sample_max, int64_t deadline_ms,
    int64_t check_start_ms, int64_t finish_reserve_ms,
    int32_t* out_sample_max, int32_t* out_budget_ms,
    int64_t* out_check_deadline_ms);
int32_t nzbget_rs_dqh_fleet_candidate_tier(
    int32_t has_error, int32_t dead, int32_t alive);
NzbgetRsBuf nzbget_rs_dqh_fleet_format_summary_log(
    int32_t members_count, const char* dupe_key, int32_t chosen,
    const char* reason, int32_t complete);
int32_t nzbget_rs_dqh_dupesearch_is_generated_key(
    int32_t dupe_key_empty, int32_t has_owner, const char* owner_value,
    int32_t nzb_id, const char* key);
int32_t nzbget_rs_dqh_dupesearch_should_check_added_for_parent_rank(
    int32_t action, int32_t ea_nzb_added, int32_t is_donor,
    int32_t dupe_mode, int32_t dm_score, int32_t has_dupe_fleet_param);
int32_t nzbget_rs_dqh_dupesearch_is_better_parent_candidate(
    int32_t is_same_item, int32_t kind, int32_t nk_nzb, int32_t deleting,
    int32_t delete_status, int32_t ds_none, int32_t has_post_info,
    int32_t same_lower_key, int32_t item_score, int32_t added_score,
    int32_t has_parent, int32_t parent_score);
int32_t nzbget_rs_dqh_dupesearch_should_retain_peer(
    int32_t is_same_item, int32_t item_score, int32_t pick_score,
    int32_t has_dupe_fleet_param);
NzbgetRsBuf nzbget_rs_dqh_dupesearch_format_rejected_outcome(
    const char* const* keys_ptr, const int32_t* counts_ptr, size_t len);
int32_t nzbget_rs_dqh_dupesearch_parse_pending_dir_id(const char* name);
int32_t nzbget_rs_dqh_dupesearch_pending_identity_matches(
    int32_t has_pick_title_file,
    const uint8_t* saved_title_ptr, size_t saved_title_len,
    const uint8_t* cur_norm_title_ptr, size_t cur_norm_title_len,
    const uint8_t* saved_key_ptr, size_t saved_key_len,
    const uint8_t* cur_lower_key_ptr, size_t cur_lower_key_len,
    const uint8_t* saved_posting_ptr, size_t saved_posting_len,
    const uint8_t* cur_fingerprint_ptr, size_t cur_fingerprint_len);
int32_t nzbget_rs_tpr_split_param_assignment(
    const char* text, size_t* out_name_len, size_t* out_value_offset);
int32_t nzbget_rs_tpr_article_fetcher_is_server_eligible_on_level(
    int32_t server_active, int32_t server_norm_level, int32_t target_level,
    int32_t server_id, int32_t server_group,
    const int32_t* failed_ids_ptr, const int32_t* failed_groups_ptr,
    const int32_t* failed_levels_ptr, size_t failed_count);
int32_t nzbget_rs_tpr_article_fetcher_is_reused_stale_within_max(
    int32_t reused, int32_t stale_count_after_inc, int32_t max_connections);
int32_t nzbget_rs_tpr_article_fetcher_is_declared_range_known(
    int64_t begin_pos, int64_t end_pos, int64_t size);
int32_t nzbget_rs_tpr_article_fetcher_should_throttle_bandwidth(
    int32_t stopped, int32_t quota_reached, int64_t speed_limit,
    int64_t cur_speed, int64_t momentary_speed);
double nzbget_rs_dqh_stream_repair_bytes_to_mb(int64_t bytes);
int32_t nzbget_rs_dqh_article_writer_use_target_path_directly(
    int32_t force_direct_write, int32_t direct_write,
    int32_t output_filename_empty, int32_t same_filename);
int32_t nzbget_rs_dqh_article_dl_is_server_available(
    int32_t active, int32_t max_connections, int32_t optional,
    int32_t is_blocked);
int32_t nzbget_rs_tpr_binrpc_is_valid_response_header(
    int32_t read_ok, int32_t host_signature, int32_t expected_signature,
    uint32_t host_struct_size, uint32_t expected_struct_size);
uint32_t nzbget_rs_tpr_binrpc_calc_elapsed_sec(
    int64_t cur_time, int64_t start_time);
int32_t nzbget_rs_tpr_binrpc_should_include_history_entry(
    int32_t kind, int32_t hk_dup, int32_t show_hidden);
int32_t nzbget_rs_tpr_ncurses_map_file_to_group_action(int32_t action);
int32_t nzbget_rs_tpr_ncurses_resolve_file_pause_action(
    int32_t action, int32_t ea_file_pause, int32_t ea_file_resume,
    int32_t file_paused);
int32_t nzbget_rs_tpr_ncurses_calc_page_nav(
    int32_t is_page_down, int32_t selected, int32_t queue_size,
    int32_t scroll_offset, int32_t win_height,
    int32_t* out_scroll_offset, int32_t* out_entry);
void nzbget_rs_tpr_options_resolve_scheduler_task_state(
    int32_t has_time, int32_t has_weekdays, int32_t has_command,
    int32_t has_download_rate, int32_t has_param_ptr,
    int32_t param_empty, int32_t process_empty, int32_t download_rate_empty,
    int32_t* out_param_source, int32_t* out_definition, int32_t* out_completed);
int32_t nzbget_rs_tpr_options_split_option_string(
    const char* option, size_t* out_name_len, size_t* out_value_offset);
void nzbget_rs_tpr_script_resolve_single_arg_executor(
    int32_t has_found, const char* found_executor, const char* arg0,
    int32_t* out_use_found, int32_t* out_prepend_arg);
NzbgetRsBuf nzbget_rs_queue_coord_format_donor_id_list(
    const int32_t* donor_ids_ptr, size_t len,
    int32_t* out_unique_ids_ptr, size_t* out_unique_len);
int32_t nzbget_rs_queue_coord_is_size_twin(
    int32_t is_self, int32_t same_name_or_key, int64_t self_size, int64_t other_size,
    int64_t other_par_size, int64_t largest_self_file,
    const int64_t* other_file_sizes_ptr, size_t other_file_sizes_len);
NzbgetRsBuf nzbget_rs_dqh_article_dl_format_connection_name(
    const char* server_name, const char* host);
NzbgetRsBuf nzbget_rs_dqh_article_dl_format_fetch_command(
    int32_t raw_article, const char* message_id);
uint32_t nzbget_rs_dqh_article_dl_select_yenc_crc(
    int32_t crc_check, uint32_t calculated_crc, uint32_t expected_crc);
const char* nzbget_rs_dqh_article_writer_open_verb(int32_t direct_write);
NzbgetRsBuf nzbget_rs_dqh_article_writer_format_info_path(
    const char* nzb_name, char sep, const char* filename);
int32_t nzbget_rs_dqh_article_writer_parent_dir_len(
    const char* filename, size_t filename_len,
    const char* final_path, size_t final_path_len,
    char sep, size_t* out_parent_len);
void nzbget_rs_dqh_article_writer_apply_missing_part_counters(
    int32_t failed_articles, int32_t success_articles,
    int32_t* out_failed_articles, int32_t* out_success_articles);
NzbgetRsBuf nzbget_rs_dqh_article_writer_format_raw_part_dest(
    const char* final_output_path, size_t final_output_path_len,
    char sep, int32_t part_number);
int32_t nzbget_rs_dqh_article_writer_calc_total_failed_articles(
    int32_t missed_articles, int32_t failed_articles);
void nzbget_rs_dqh_statmeter_calc_debug_speed_and_timediff(
    int32_t current_download_speed, int32_t cur_time,
    int32_t speed_start_time, int32_t slot_size,
    int32_t* out_speed_kb, int32_t* out_time_diff);
int32_t nzbget_rs_dqh_statmeter_bytes_to_mb_i32(int64_t bytes);
NzbgetRsBuf nzbget_rs_tpr_binrpc_format_download_response_msg(
    int32_t ok, const char* base_filename);
uint32_t nzbget_rs_tpr_binrpc_eval_nzb_group_match(
    int32_t match_group, int32_t has_regex, int32_t regex_matched);
NzbgetRsBuf nzbget_rs_tpr_binrpc_format_file_match_name(
    const char* nzb_name, const char* base_filename);
NzbgetRsBuf nzbget_rs_tpr_web_format_quoted_hex_etag(size_t hash);
int32_t nzbget_rs_tpr_web_is_gzip_smaller(int32_t gzipped_len, int32_t body_len);
NzbgetRsBuf nzbget_rs_tpr_web_format_single_file_path(
    const char* web_dir, const char* url, const char* def_res);
int32_t nzbget_rs_tpr_xmlrpc_split_url_path_and_query(
    const char* url, size_t* out_path_set_len, size_t* out_query_offset);
int32_t nzbget_rs_tpr_xmlrpc_should_use_edit_offset(
    int32_t has_offset, int32_t action,
    int32_t ea_file_move_offset, int32_t ea_group_move_offset);
NzbgetRsBuf nzbget_rs_tpr_xmlrpc_format_readurl_temp_filename(
    const char* temp_dir, char sep, int32_t num);
NzbgetRsBuf nzbget_rs_tpr_xmlrpc_format_testserver_article_cmd(const char* id);
NzbgetRsBuf nzbget_rs_tpr_post_format_info_name(
    const char* prefix, const char* name);
int32_t nzbget_rs_dqh_conn_calc_read_buffer_cap(int32_t read_buf_size, int32_t decode_slack);
NzbgetRsBuf nzbget_rs_dqh_conn_format_port_str(int32_t port);
int32_t nzbget_rs_dqh_conn_clamp_buf_avail(int32_t buf_avail);
NzbgetRsBuf nzbget_rs_dqh_conn_format_report_err_with_code(
    const char* prefix, int32_t err_code, const char* err_msg);
NzbgetRsBuf nzbget_rs_dqh_tls_format_report_err(const char* err_msg, const char* err_str);
NzbgetRsBuf nzbget_rs_dqh_tls_format_cert_verify_err(
    const char* host, const char* verify_err_str);
NzbgetRsBuf nzbget_rs_dqh_tls_format_handshake_err(const char* host);
NzbgetRsBuf nzbget_rs_dqh_tls_format_hostname_mismatch_err(
    const char* host, const char* cert_host);
const char* nzbget_rs_dqh_tls_select_io_err_msg(int32_t is_write, int32_t peek_err_zero);
NzbgetRsBuf nzbget_rs_tpr_webdl_format_get_request_line(const char* resource);
NzbgetRsBuf nzbget_rs_tpr_webdl_format_user_agent_header(const char* version_rev);
NzbgetRsBuf nzbget_rs_tpr_webdl_format_host_header(
    int32_t is_default_port, const char* host, int32_t port);
int32_t nzbget_rs_tpr_webdl_kb_to_bytes(int32_t kb);
const char* nzbget_rs_tpr_ncurses_format_messages_header(int32_t use_color);
NzbgetRsBuf nzbget_rs_tpr_ncurses_format_timestamp_message(
    const char* time_str, const char* msg_text);
NzbgetRsBuf nzbget_rs_tpr_ncurses_format_download_rate_hint(int32_t input_val);
const char* nzbget_rs_tpr_ncurses_pause_log_msg(int32_t pause_download);
NzbgetRsBuf nzbget_rs_dqh_maintenance_resolve_script_path(
    const char* app_dir, char sep, const char* script);
NzbgetRsBuf nzbget_rs_dqh_maintenance_format_package_info_path(
    const char* web_dir, char sep);
NzbgetRsBuf nzbget_rs_dqh_maintenance_format_script_info_name(
    int32_t is_info_script, const char* base_name);
const char* nzbget_rs_dqh_maintenance_branch_name(int32_t branch);
const char* nzbget_rs_dqh_maintenance_run_mode_name(int32_t daemon_mode);
NzbgetRsBuf nzbget_rs_dqh_maintenance_format_cmdline_env_name(int32_t index);
NzbgetRsBuf nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
    const char* base_name, size_t max_len, size_t* out_prefix_len);
NzbgetRsBuf nzbget_rs_dqh_maintenance_format_sig_title(const char* base_name);
int32_t nzbget_rs_dqh_scheduler_bytes_to_kb(int32_t bytes);
const char* nzbget_rs_dqh_scheduler_pause_action_str(int32_t paused);
const char* nzbget_rs_dqh_scheduler_server_action_str(int32_t active);
NzbgetRsBuf nzbget_rs_tpr_article_writer_format_dest_file_path(
    const char* dest_dir, char sep, const char* filename);
NzbgetRsBuf nzbget_rs_tpr_article_writer_format_tmp_path(const char* base_path);
size_t nzbget_rs_tpr_article_cache_mb_to_bytes(int32_t mb);
size_t nzbget_rs_tpr_server_volume_required_day_len(int32_t day_slot, size_t current_len);
NzbgetRsBuf nzbget_rs_tpr_options_format_default_conf_path(const char* dir, char sep);
size_t nzbget_rs_tpr_options_strip_trailing_sep(const char* dir, char sep);
NzbgetRsBuf nzbget_rs_tpr_options_format_joined_dir(
    const char* parent_dir, char sep, const char* dir, int32_t join_mode);
int32_t nzbget_rs_tpr_options_minutes_to_seconds_i32(int32_t minutes);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_new_path(const char* base_path);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_ids_line(int32_t nzb_id, int32_t file_id);
const char* nzbget_rs_dqh_diskstate_legacy_suffix_str(int32_t suffix_kind);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_unreadable_path(const char* filename, int64_t now);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_out_tmp_path(
    const char* dest_dir, char sep, int32_t file_id);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_staged_part_path(
    const char* temp_dir, char sep, int32_t file_id, int32_t part_num);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_file_state_name(int32_t file_id, int32_t completed);
NzbgetRsBuf nzbget_rs_dqh_diskstate_format_nzb_log_path(
    const char* queue_dir, char sep, int32_t nzb_id);
NzbgetRsBuf nzbget_rs_tpr_diskstate_format_queue_file_state_path(
    const char* queue_dir, char sep, const char* filename, char suffix);
NzbgetRsBuf nzbget_rs_tpr_diskstate_format_queue_id_state_path(
    const char* queue_dir, char sep, int32_t file_id, char suffix);
NzbgetRsBuf nzbget_rs_queue_coord_format_par_damage_summary(
    int32_t certain, int32_t damaged_blocks, int32_t projected_blocks,
    int64_t block_size, int32_t recovery_blocks);
NzbgetRsBuf nzbget_rs_dqh_unpack_ctrl_strip_last_extension(const char* frag_base_name);
NzbgetRsBuf nzbget_rs_dqh_article_fetcher_format_body_cmd(const char* message_id);
NzbgetRsBuf nzbget_rs_dqh_webserver_format_forwarded_for_suffix(const char* forwarded_for);
NzbgetRsBuf nzbget_rs_dqh_newznab_format_search_info_name(
    const char* label, int32_t page_one_based);
NzbgetRsBuf nzbget_rs_dqh_dupesearch_format_key_val_param(
    const char* param_name, int32_t val);
NzbgetRsBuf nzbget_rs_dqh_dupesearch_format_pending_dir(
    const char* queue_dir, char sep, int32_t nzb_id);
NzbgetRsBuf nzbget_rs_dqh_dupesearch_format_indexed_base_path(
    const char* dir, char sep, size_t idx);
NzbgetRsBuf nzbget_rs_dqh_dupesearch_format_usize_str(size_t idx);
NzbgetRsBuf nzbget_rs_dqh_nzb_fetcher_format_http_status_err(int32_t status);
NzbgetRsBuf nzbget_rs_dqh_http_get_format_temp_path(
    const char* temp_dir, char sep, int32_t now_sec, int32_t counter);
int64_t nzbget_rs_dqh_fleet_calc_check_lock_wait_ms(
    int64_t deadline_ms, int64_t now_ms, int64_t finish_reserve_ms);
NzbgetRsBuf nzbget_rs_dqh_script_ctrl_format_env_assignment(
    const char* name, const char* value);
NzbgetRsBuf nzbget_rs_dqh_script_ctrl_format_prefixed_varname(
    const char* prefix, const char* name);
NzbgetRsBuf nzbget_rs_dqh_log_format_debug_line(
    const char* msg, const char* base_filename, int32_t line_nr, const char* funcname);
NzbgetRsBuf nzbget_rs_dqh_log_format_rotate_mask(
    const char* base_name, const char* base_ext);
NzbgetRsBuf nzbget_rs_dqh_log_format_deleting_old_msg(const char* filename);
NzbgetRsBuf nzbget_rs_dqh_log_format_rotated_path(
    const char* directory, char sep, const char* base_name,
    int32_t year, int32_t month, int32_t mday, const char* base_ext);
NzbgetRsBuf nzbget_rs_dqh_options_format_indexed_opt_name(
    const char* prefix, int32_t idx, const char* field);
NzbgetRsBuf nzbget_rs_dqh_nzbget_format_pid_line(int32_t pid);
NzbgetRsBuf nzbget_rs_dqh_ncurses_format_plot_line(
    const char* text, int32_t screen_width, int32_t pos, int32_t max_screen_width);
NzbgetRsBuf nzbget_rs_dqh_fs_format_expanded_rel_path(
    const char* cur_dir, const char* rel_filename);
NzbgetRsBuf nzbget_rs_dqh_benchmark_format_unique_filename(int64_t epoch_ticks);
size_t nzbget_rs_dqh_benchmark_resolve_block_size(
    size_t block_size_bytes, size_t default_block_size);
NzbgetRsBuf nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
    int32_t first_idx, const int64_t* values, size_t count);
NzbgetRsBuf nzbget_rs_dqh_sysinfo_format_rapidyenc_version(int32_t ver);
NzbgetRsBuf nzbget_rs_dqh_fs_format_null_delete_errmsg(const char* last_err);
NzbgetRsBuf nzbget_rs_dqh_fs_extract_parent_dir_path(const char* filename, char sep);
int nzbget_rs_dqh_diskstate_is_candidate_queue_file_name(const char* filename);
int nzbget_rs_dqh_diskstate_find_char_pos(
    const char* line, char sep, size_t* out_pos);
int nzbget_rs_dqh_tls_is_tls13_version_str(const char* ver);
uint32_t nzbget_rs_dqh_tls_resolve_cipher_bits(const char* cipher_name, uint32_t raw_bits);
NzbgetRsBuf nzbget_rs_dqh_tls_format_bulk_cipher_name(
    const char* bulk_name, const char* cipher_name);
uint32_t nzbget_rs_dqh_tls_compute_suite_flags(
    int is_aead, int tls13, int auth_is_cert, int kx_is_ephemeral);
int nzbget_rs_dqh_nntp_conn_is_already_in_group(
    const char* active_group, const char* grp);
int nzbget_rs_dqh_options_should_expand_home_value(
    const char* value, int no_disk_access);
int nzbget_rs_dqh_nzbget_is_backward_compat_root_user(const char* username);
int nzbget_rs_dqh_xmlrpc_is_multicall_post(
    const char* method_name, int is_xmlrpc_proto, int is_http_post);
int nzbget_rs_dqh_xmlrpc_is_tls_cipher_baseline_entry(const char* name);
int nzbget_rs_dqh_webserver_is_html_content_type(const char* content_type);
int nzbget_rs_dqh_scanner_find_last_char_pos(
    const char* text, char ch, size_t* out_pos);
NzbgetRsBuf nzbget_rs_dqh_log_split_basename_and_ext(
    const char* base_name, NzbgetRsBuf* out_ext);
int nzbget_rs_dqh_sysinfo_line_contains_marker(
    const char* line, const char* marker);
int nzbget_rs_dqh_bytes_equal(const char* a, const char* b);
int nzbget_rs_dqh_bytes_eq_ignore_case(const char* a, const char* b);
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
int nzbget_rs_dqh_tls_resolve_probe_result(
    int connected, const char* negotiated_cipher, const char* expected_cipher,
    int negotiated_version, int protocol, int tls13_ver, int tls12_ver,
    int auth_error, int tls_probe_failure);
int nzbget_rs_dqh_maintenance_match_sig_line_prefix(
    const char* line, const char* sig_title, size_t* out_offset);
int64_t nzbget_rs_dqh_parse_c_atoll(const char* text);
int nzbget_rs_dqh_parse_c_atoi(const char* text);
int nzbget_rs_dqh_parse_c_atoi_or_default(const char* text, int default_val);
int nzbget_rs_dqh_diskstate_parse_queue_file_id_and_suffix(
    const char* filename, int* out_id, char* out_suffix);
int nzbget_rs_dqh_nstring_find_substr_from(
    const char* haystack, const char* needle, int pos);
void nzbget_rs_dqh_log_parse_rotated_date_fields(
    const char* filename, size_t year_off, size_t mon_off, size_t mday_off,
    int* out_year, int* out_mon, int* out_mday);
int nzbget_rs_dqh_mem_slices_equal(const void* a, const void* b, size_t len);
int nzbget_rs_dqh_tls_is_cipher_list(const char* cipher, size_t cipher_len);
NzbgetRsBuf nzbget_rs_dqh_sysinfo_extract_version_after_space(const char* text);
int nzbget_rs_dqh_os_classify_os_release_line(
    const char* line, size_t line_len, int has_name, int has_version,
    size_t* out_val_offset);
int nzbget_rs_dqh_dupe_util_trim_bounds(
    const char* text, size_t text_len, const char* chars,
    size_t* out_start, size_t* out_len);
NzbgetRsBuf nzbget_rs_dqh_dupe_util_lower(const char* text, size_t text_len);
NzbgetRsBuf nzbget_rs_dqh_nzb_fetcher_format_parse_err(
    const char* body, size_t body_len);
int nzbget_rs_dqh_util_classify_executor_ext(
    const char* filename, size_t filename_len, size_t* out_dot_idx);
int nzbget_rs_dqh_slice_eq_ignore_case(
    const char* a, size_t a_len, const char* b, size_t b_len);
int nzbget_rs_dqh_options_should_check_single_script_dir(
    int no_disk_access, const char* value);
void nzbget_rs_dqh_webserver_encode_hex_token(
    const uint8_t* bytes, size_t bytes_len, char* out_token, size_t out_cap);
NzbgetRsBuf nzbget_rs_dqh_script_format_prefixed_message(
    const char* prefix, const char* text);
NzbgetRsBuf nzbget_rs_dqh_xmlrpc_concat_decoded_path_and_query(
    const char* decoded_path, const char* url, int has_query, size_t query_offset);
int nzbget_rs_dqh_diskstate_next_comma_token(
    const char* s, size_t cursor, size_t* out_tok_start,
    size_t* out_tok_len, size_t* out_next_cursor);
char* nzbget_rs_dqh_tokenizer_next(
    char* data_string, const char* separators, int working, char** save_ptr);
const char* nzbget_rs_dqh_scheduler_command_name(int command);
const char* nzbget_rs_dqh_frontend_resolve_control_ip(const char* control_ip);
const char* nzbget_rs_dqh_ncurses_message_type_label(int kind, int first_line);
int nzbget_rs_dqh_ncurses_message_color_pair(int kind);
const char* nzbget_rs_dqh_xmlrpc_response_content_type(int is_xml_rpc);
const char* nzbget_rs_dqh_xmlrpc_test_server_speed_err_msg(int server_err);
const char* nzbget_rs_dqh_binrpc_write_log_response_msg(int ok);
const char* nzbget_rs_dqh_binrpc_scan_response_msg(int sync_mode);
uint32_t nzbget_rs_dqh_article_writer_step_file_crc(
    int first_article, uint32_t current_crc, uint32_t article_crc,
    int segment_size, int* out_first_article);
void nzbget_rs_dqh_post_capitalize_first_ascii(char* s);
const char* nzbget_rs_dqh_dupesearch_prepare_status_suffix(int rank_only, int history);
void nzbget_rs_dqh_dupesearch_visit_expand_queries(
    const char* name, size_t name_len, void* ctx,
    void (*cb)(void* ctx, const char* q_ptr, size_t q_len));
NzbgetRsBuf nzbget_rs_dqh_newznab_format_query_label(
    const char* const* keys, const char* const* vals, size_t count);
NzbgetRsBuf nzbget_rs_dqh_nntp_health_format_bracketed_id(
    const char* message_id, size_t len);
NzbgetRsBuf nzbget_rs_dqh_nntp_health_format_stat_batch(
    const char* const* ids, const size_t* lens, size_t count);
int nzbget_rs_dqh_article_writer_check_existing_output_file(
    int exists, int64_t existing_size, int64_t target_size);
int nzbget_rs_dqh_article_writer_select_direct_output_mode(
    int direct_write, int force_direct_write, int has_existing_output,
    int output_initialized, int dest_exists);
int nzbget_rs_dqh_article_writer_should_fallback_open_to_final_path(
    int open_exists, int final_exists);
int nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
    int64_t actual_size, int segment_size);
int nzbget_rs_dqh_article_writer_eval_commit_post_close(
    int flushed, int closed, int direct_write);
int nzbget_rs_dqh_article_writer_is_staged_dupe_article_file(
    int dupe_fallback_round, int has_result_filename);
int nzbget_rs_dqh_article_writer_classify_direct_final_cleanup(
    int final_empty, int same_filename, int output_exists);
int nzbget_rs_dqh_article_cache_should_write_cache_flag(
    int was_empty, int server_mode, int continue_partial, int skip_write);
int nzbget_rs_dqh_article_cache_post_flush_action(
    int need_emergency_flush, size_t allocated, int just_flushed, int is_stopped);
int nzbget_rs_dqh_article_fetcher_should_clear_unreachable(
    int success, int transient);
int nzbget_rs_dqh_article_fetcher_can_attempt_transient_retry(
    int transient, int server_unreachable);
int nzbget_rs_dqh_article_fetcher_has_more_transient_attempts(
    int attempt, int max_transient_attempts);
int nzbget_rs_dqh_article_batch_can_store_result(
    int batch_id, int current_batch_id, int cancelled);
int nzbget_rs_dqh_article_batch_can_deliver_next(
    int cancelled, int stopped, size_t next_deliver, size_t total_requests);
int nzbget_rs_dqh_webserver_classify_pre_parse_method(
    int http_method, int hm_post, int hm_options, int content_len);
int nzbget_rs_dqh_binrpc_classify_history_entry_fields(
    int kind, int hk_nzb, int hk_dup, int hk_url, int show_hidden);
int nzbget_rs_dqh_diskstate_classify_begin_read_recovery(
    int transactional, int dest_exists, int temp_exists);
int nzbget_rs_dqh_diskstate_select_volume_server_slot(
    int index, const int32_t* state_ids, const int32_t* server_ids, size_t count);
int nzbget_rs_dqh_xmlrpc_classify_tls_cipher_op(int operation);
int nzbget_rs_dqh_xmlrpc_calc_schedule_resume_time(
    int has_param, int seconds, int64_t cur_time, int64_t* out_resume_time);
void nzbget_rs_dqh_xmlrpc_accum_queue_job_counts(
    int has_post_info, int kind, int nk_url,
    int* inout_post_count, int* inout_url_count);
int nzbget_rs_dqh_xmlrpc_is_fleet_member_limit_reached(
    size_t current_count, int max_members);
int nzbget_rs_dqh_xmlrpc_classify_history_entry_kind(
    int kind, int hk_nzb, int hk_url, int hk_dup);
int nzbget_rs_dqh_xmlrpc_matches_edit_server_id(int server_id, int target_id);
int nzbget_rs_dqh_xmlrpc_matches_reset_server_volume(int index, int server_id);
int nzbget_rs_dqh_xmlrpc_is_valid_cert_verif_level(int level, int count);
const char* nzbget_rs_dqh_xmlrpc_select_test_server_err_text(
    int ok, const char* err_text);
int nzbget_rs_dqh_xmlrpc_check_disk_speed_arg_ranges(
    int write_buffer_kib, int max_file_size_gib, int timeout_sec);
int nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
    int index, int scroll_offset, int win_height);
void nzbget_rs_dqh_ncurses_accum_file_queue_stats(
    int paused, int64_t file_rem_size,
    int64_t* inout_rem, int64_t* inout_paused, int* inout_paused_files);
void nzbget_rs_dqh_ncurses_accum_group_queue_stats(
    int64_t group_rem_size, int64_t group_paused_size,
    int64_t* inout_rem, int64_t* inout_paused);
int nzbget_rs_dqh_ncurses_enter_edit_queue_win_pct(
    int queue_size, int cur_win_pct, int* out_win_pct);
int nzbget_rs_dqh_ncurses_last_queue_index(int queue_size);
int nzbget_rs_dqh_disk_service_should_check_disk_space(
    int disk_space_mb, int is_downloading);
int nzbget_rs_dqh_disk_service_eval_missing_required_dir(
    int file_exists, int dir_exists, int was_waiting_reported, int cur_waiting_reported,
    int* out_should_log_waiting, int* out_next_waiting_reported);
int nzbget_rs_dqh_maintenance_has_update_info(int len);
int nzbget_rs_dqh_options_resolve_locate_src_line(int found, int line_no);
int nzbget_rs_dqh_options_update_config_line_on_get(int line_no, int cur_config_line);
int nzbget_rs_dqh_options_is_every_hour_task(int hours);
int nzbget_rs_dqh_nzbget_should_run_system_health_check(
    int server_mode, int system_health_check);
int nzbget_rs_dqh_nzbget_classify_boot_usage_or_error(
    int print_usage, int has_errors, int arg_count, int* out_exit_code);
int nzbget_rs_dqh_nzbget_should_stop_frontend_immediately(
    int remote_client_mode, int stopped);
int nzbget_rs_dqh_log_classify_init_logfile_action(
    int remote_client_mode, int direct_client_mode, int write_log,
    int wl_none, int wl_reset, int has_log_file, int server_mode);
int nzbget_rs_dqh_script_ctrl_is_terminate_ok(
    int child_pid, int kill_res, int completed);
int nzbget_rs_dqh_script_ctrl_can_send_break(int pid, int completed);
int nzbget_rs_dqh_fs_should_fallback_move_to_copy(
    int rename_ok, int err_no, int exdev);
int nzbget_rs_dqh_fs_is_copied_file_valid(
    int close_ok, int loop_ok, int64_t copied_bytes, int64_t src_size);
int nzbget_rs_dqh_par_renamer_needs_create_target_dir(
    const char* old_path, const char* new_path);
int nzbget_rs_dqh_dupematcher_should_wait_rar_lister(
    int is_running, int64_t start_time, int timeout_sec, int64_t cur_time);
int nzbget_rs_dqh_article_dl_is_terminal_loop_status(
    int status, int ad_finished, int ad_fatal_error);
int nzbget_rs_dqh_article_dl_should_fail_for_desired_server(
    int has_want_server, int has_desired_server);
int nzbget_rs_dqh_article_dl_should_continue_read_loop(int is_stopped, int decoder_eof);
int nzbget_rs_dqh_article_dl_should_run_decode_check(int status, int ad_running);
int nzbget_rs_dqh_server_pool_has_available_level_conns(
    int level, size_t levels_len, int level_free_count);
size_t nzbget_rs_dqh_server_pool_pick_candidate_index(int rand_val, size_t candidates_len);
int nzbget_rs_dqh_server_pool_should_disconnect_idle_level_conn(
    int conn_norm_level, int level, int conn_status, int cs_connected);
int nzbget_rs_dqh_statmeter_has_any_quota_configured(int daily_quota, int monthly_quota);
int nzbget_rs_dqh_statmeter_is_before_quota_start_day(int quota_start_day, int cur_mday);
int nzbget_rs_dqh_statmeter_did_prev_month_day_overflow(int prev_mday, int quota_start_day);
int nzbget_rs_dqh_nntp_conn_classify_handshake_result(int result);
int nzbget_rs_dqh_article_fetcher_should_keep_waiting_retry(
    int waited, int seconds, int is_stopped);
int nzbget_rs_dqh_article_fetcher_should_disconnect_on_release(
    int keep_connected, int conn_status, int cs_cancelled);
int nzbget_rs_dqh_article_fetcher_is_join_group_failure_transient(int has_response);
int nzbget_rs_dqh_article_batch_should_ensure_workers(int worker_count, int is_stopped);
int nzbget_rs_dqh_article_writer_is_dupe_fallback_article(
    int has_article_info, int dupe_fallback_round);
int nzbget_rs_dqh_article_writer_was_renamed_during_completion(
    const char* cur_filename, const char* orig_filename, size_t orig_len);
int nzbget_rs_dqh_article_writer_should_delete_hardlinked_without_interdir(
    int is_hard_linked, int inter_dir_empty);
int nzbget_rs_dqh_article_writer_did_dest_dir_change(
    const char* cur_dest_dir, const char* orig_dest_dir, size_t orig_len);
int nzbget_rs_dqh_article_cache_should_continue_run_loop(int is_stopped, size_t allocated);
int nzbget_rs_dqh_memory_monitor_should_keep_sleeping(
    int is_stopped, int slept, int interval);
int nzbget_rs_dqh_webdl_should_continue_retry_loop(
    int is_stopped, int remained_download_retries, int remained_connect_retries);
int nzbget_rs_dqh_webdl_should_keep_waiting_retry_interval(
    int is_stopped, int msec, int url_interval_sec, int force, int pause_download);
int nzbget_rs_dqh_webdl_is_terminal_retry_status(
    int status, int ad_finished, int ad_fatal_error, int ad_not_found);
int nzbget_rs_dqh_conn_is_unix_path_too_long(size_t host_len, size_t sun_path_cap);
int nzbget_rs_dqh_conn_resolve_gai_report_err_code(int res, int eai_system);
int nzbget_rs_dqh_conn_should_report_socket_create_failed(
    int is_invalid_socket, int has_addr_list);
int nzbget_rs_dqh_conn_should_cleanup_unconnected_socket(
    int connected, int is_invalid_socket);
int nzbget_rs_dqh_conn_should_unlink_listening_unix_socket(
    int is_unix_path, int getsockopt_res, int is_listening);
int nzbget_rs_dqh_conn_resolve_report_err_code(int err_code, int last_net_err);
int nzbget_rs_dqh_tls_has_cert_and_key_files(int cert_empty, int key_empty);
int nzbget_rs_dqh_tls_should_set_sni_hostname(int is_client, int host_empty);
int nzbget_rs_dqh_tls_should_validate_client_cert(int is_client, int should_verify);
int nzbget_rs_dqh_tls_is_proto_version_out_of_bounds(
    int version, int minimum, int maximum);
int nzbget_rs_dqh_tls_should_send_close_notify(
    int has_session, int connected, int has_deadline);
int nzbget_rs_dqh_webdl_is_connect_aborted(int connected, int is_stopped);
int nzbget_rs_dqh_webdl_should_download_body(int status, int ad_running);
int nzbget_rs_dqh_webdl_should_delete_broken_output(int status, int ad_finished);
int nzbget_rs_dqh_webdl_needs_prepare_file(int out_file_active);
int nzbget_rs_dqh_webdl_is_gunzip_write_failed(int out_len, int64_t written_bytes);
int nzbget_rs_dqh_webdl_should_disconnect_on_free(int conn_status, int cs_cancelled);
int nzbget_rs_dqh_speed_test_is_iteration_failed(
    double elapsed_sec, uint32_t downloaded, uint32_t target_size);
int nzbget_rs_dqh_speed_test_should_stop_recv_loop(
    int received, double elapsed_sec, double timeout_sec);
void nzbget_rs_dqh_tls_cipher_fill_test_plaintext(uint8_t* buf, size_t len);
int nzbget_rs_dqh_tls_cipher_is_prepare_batch_valid(uint64_t sequence, uint32_t count);
void nzbget_rs_dqh_tls_cipher_write_record_nonce(
    const uint8_t* prefix4, uint64_t seq, uint8_t* out_nonce12);
int nzbget_rs_dqh_tls_cipher_is_record_seq_in_range(
    uint64_t sequence, uint64_t first, size_t count);
int nzbget_rs_dqh_tls_cipher_precheck_probe_args(
    int cancelled, uint64_t milliseconds, int verification, int has_cert_store);
int nzbget_rs_dqh_tls_cipher_is_probe_addr_list_valid(size_t addr_count);
int nzbget_rs_dqh_diskstate_resolve_read_version(
    int read_version, int format_version);
int nzbget_rs_dqh_diskstate_state_file_exists(
    int dest_exists, int transactional, int temp_exists);
int nzbget_rs_dqh_diskstate_should_flush_queue_file(
    int flush, int flush_queue_opt);
int nzbget_rs_dqh_diskstate_is_file_version_unsupported(
    int file_version, int read_version);
int nzbget_rs_dqh_diskstate_should_load_separate_history_file(
    int format_version);
int nzbget_rs_dqh_diskstate_should_reload_fallback_file_infos(
    int file_infos_loaded, int queue_empty);
int nzbget_rs_dqh_diskstate_expected_repair_job_fields(int format_version);
int nzbget_rs_dqh_diskstate_is_nonneg_count_scan_invalid(
    int scan_cnt, int count);
int nzbget_rs_dqh_diskstate_has_meta_name_field(int format_version);
int nzbget_rs_dqh_diskstate_matches_server_state_id(
    int news_server_state_id, int server_id);
const char* nzbget_rs_dqh_diskstate_select_original_or_current_msg_id(
    const char* orig_msg_id, const char* cur_msg_id);
int nzbget_rs_dqh_diskstate_should_probe_direct_write_files(
    int output_initialized, int direct_write_opt, int force_direct_write);
int nzbget_rs_dqh_diskstate_can_check_completed_dest_file(
    int completed, int force_direct_write, int dest_dir_empty,
    int filename_empty);
int nzbget_rs_dqh_diskstate_matches_history_nzb_id(
    int kind, int hk_nzb, int hk_url, int nzb_id, int target_id);
int nzbget_rs_dqh_diskstate_should_discard_feeds_file(
    int feeds_empty, int feed_history_empty);
int nzbget_rs_dqh_diskstate_can_store_volume_slot(
    int has_volume_array, int index, size_t array_size);
int nzbget_rs_dqh_webdl_should_try_recv_when_buf_empty(int buf_len);
int nzbget_rs_dqh_tls_should_verify_client_x509_store(
    int is_client, int has_x509_store);
int nzbget_rs_dqh_tls_should_check_verify_result_on_handshake_fail(
    int is_client, int should_verify);
int nzbget_rs_dqh_tls_cipher_classify_probe_deadline_override(
    int current_result, int is_cancelled, int deadline_expired);
int nzbget_rs_dqh_frontend_should_request_file_list(
    int summary, int file_list);
int nzbget_rs_dqh_frontend_should_build_remote_file_list(
    int file_list, uint32_t trailing_data_len);
int nzbget_rs_dqh_ncurses_should_reset_empty_edit_queue(
    int is_edit_queue, int queue_size);
int nzbget_rs_dqh_remote_server_is_cert_or_key_missing(
    const char* path, int file_exists);
int nzbget_rs_dqh_remote_server_should_reset_listener(
    int bind_ok, int has_accepted_conn);
int nzbget_rs_dqh_remote_client_is_empty_edit_target(
    size_t id_count, size_t name_count);
int nzbget_rs_dqh_webserver_classify_ext_route(int get_route);
int nzbget_rs_dqh_webserver_is_rel_path_outside_root(
    int is_empty, char first_char);
int nzbget_rs_dqh_binrpc_should_lookup_unusual_file_owner(
    int same_nzb_ptr);
int nzbget_rs_dqh_binrpc_should_match_file_regex(
    int has_regex, int match_group);
int nzbget_rs_dqh_xmlrpc_is_json_protocol(
    int protocol, int rp_json_rpc, int rp_jsonp_rpc);
int nzbget_rs_dqh_xmlrpc_should_skip_to_json_post_params(
    int is_json, int http_method, int hm_post);
int nzbget_rs_dqh_xmlrpc_should_check_queue_script_job(
    int has_post_info, int post_stage, int pt_queued);
int nzbget_rs_dqh_xmlrpc_classify_append_result_mode(
    int v13, int nzb_id);
int nzbget_rs_dqh_xmlrpc_should_emit_json_array_comma(
    int is_json, int index);
int nzbget_rs_dqh_xmlrpc_should_parse_json_post_test_server(
    int is_json, int has_request, int http_method, int hm_post);
int nzbget_rs_dqh_xmlrpc_classify_start_script_param_mode(
    int is_json, int has_p1, int has_p2, int has_p3, int has_p4);

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
    const int* tolower_table,
    int char_is_signed,
    int (*fold)(int),
    NzbgetRsParDamageVerdict* out_verdict);

int nzbget_rs_par_damage_first_report(int nzb_id);
void nzbget_rs_par_damage_forget(int nzb_id);

void nzbget_rs_dupematcher_find_largest_file(
    const char* directory,
    char* filename_buf,
    int buf_len,
    int64_t* out_max_size,
    int* out_compressed,
    int aliases_member_max,
    int64_t expected_rar_size,
    void (*rar_lister_cb)(void* ctx, char* filename_buf, int buf_len, int64_t expected_size, int64_t* out_max_size, int* out_compressed),
    void* rar_lister_ctx);

const char* nzbget_rs_dupematcher_size_mismatch_suffix(int ok);
int nzbget_rs_dupematcher_is_content_match(
    int64_t dupe_max_size, int64_t max_size, int dupe_compressed, int compressed);

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

int nzbget_rs_move_controller_decide_outcome(
    int ok, int is_stopped, int* out_move_status);

int nzbget_rs_cleanup_controller_run(
    const char* dest_dir,
    const char* final_dir,
    const char* ext_cleanup_disk,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg),
    int* out_cleanup_status);

const char* nzbget_rs_rename_controller_archive_kind_prefix(int kind, int jk_rar);
int nzbget_rs_rename_controller_classify_run_outcome(int is_stopped, int renamed_count);
int nzbget_rs_rename_controller_eval_par_config(
    int unpack_status, int us_success, int us_none, int final_dir_empty,
    int* out_detect_missing);
int nzbget_rs_rename_controller_complete_action(
    int kind, int renamed_count, int has_damaged_pars, int remaining_par_count,
    int has_missed_files, int par_status,
    int* out_request_extra_pars, int* out_request_par_check);
int nzbget_rs_rename_controller_should_reset_par_status(int64_t remaining_size);

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

void nzbget_rs_archive_processor_handle_extract_failure(
    const char* archive_file,
    const char* unpack_dir,
    const char* broken_dir,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg));

void nzbget_rs_archive_processor_finish(
    const char* archive_file,
    const char* unpack_dir,
    const char* dest_dir,
    const char* processed_dir,
    int config_action,
    void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*moved_nzb_cb)(void* ctx, const char* moved_path));

typedef void (*NzbgetRsHealthOptionEmitCb)(
    void* ctx, const char* name_ptr, size_t name_len,
    int severity, const char* msg_ptr, size_t msg_len);

const char* nzbget_rs_health_severity_to_str(int severity);
void nzbget_rs_health_validate_paths_section(
    const char* main_dir, size_t main_dir_len,
    const char* dest_dir, size_t dest_dir_len,
    const char* inter_dir, size_t inter_dir_len,
    const char* nzb_dir, size_t nzb_dir_len,
    const char* queue_dir, size_t queue_dir_len,
    const char* web_dir, size_t web_dir_len,
    const char* temp_dir, size_t temp_dir_len,
    const char* const* script_dir_ptrs, const size_t* script_dir_lens, size_t script_dir_count,
    const char* config_template, size_t config_template_len,
    const char* log_file, size_t log_file_len,
    const char* cert_store, size_t cert_store_len,
    const char* cert_store_path, size_t cert_store_path_len,
    const char* config_file, size_t config_file_len,
    const char* lock_file, size_t lock_file_len,
    int write_log, int cert_check, int daemon_mode,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_category_section(
    const char* name, size_t name_len, int global_unpack, int cat_unpack,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
int nzbget_rs_health_validate_duplicate_category_names(
    const char* const* name_ptrs, const size_t* name_lens, size_t count,
    NzbgetRsBuf* out_msg);
void nzbget_rs_health_validate_display_section(
    int output_mode, int update_interval, int curses_nzb_name,
    int curses_group, int curses_time,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_logging_section(
    int write_log, const char* log_file, size_t log_file_len,
    int rotate_log, int log_buffer, int crash_dump, int time_correction,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_scheduler_task_section(
    int hours, int minutes, int weekdays_bits,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_feed_section(
    const char* name, size_t name_len, const char* url, size_t url_len,
    int interval, uint32_t cert_level,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
int nzbget_rs_health_validate_duplicate_feed_urls(
    const char* const* name_ptrs, const size_t* name_lens,
    const char* const* url_ptrs, const size_t* url_lens,
    size_t count, NzbgetRsBuf* out_msg);
void nzbget_rs_health_validate_incoming_nzb_section(
    int append_category_dir, int nzb_dir_interval, int nzb_dir_file_age,
    int dupe_check, int dupe_search, int health_check,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_unpack_section(
    int unpack, int direct_unpack, int unpack_pause_queue,
    const char* unrar_path, size_t unrar_path_len,
    const char* sevenzip_path, size_t sevenzip_path_len,
    const char* ext_cleanup_disk, size_t ext_cleanup_disk_len,
    const char* unpack_pass_file, size_t unpack_pass_file_len,
    const char* unpack_ignore_ext, size_t unpack_ignore_ext_len,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_check_and_repair_section(
    int crc_check, int par_check, int par_repair, int par_quick,
    int par_buffer, int par_threads, size_t hw_threads,
    int par_rename, int rar_rename, int direct_rename, int hard_linking,
    int health_check, int par_time_limit,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_download_queue_section(
    int flush_queue, int continue_partial, int propagation_delay,
    int article_cache, int direct_write, int write_buffer,
    int file_naming, int post_strategy, int disk_space,
    int nzb_cleanup_disk, int keep_history, int feed_history,
    int skip_write, int raw_article,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_connection_section(
    int article_retries, int hc_dupe, int dupe_search,
    int article_interval, int article_timeout, int article_read_chunk_size,
    int url_retries, int url_interval, int url_timeout,
    int url_connections, int remote_timeout, int download_rate,
    int monthly_quota, int quota_start_day, int daily_quota,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_security_section(
    const char* control_ip, size_t control_ip_len,
    int control_port,
    const char* control_username, size_t control_username_len,
    const char* control_password, size_t control_password_len,
    const char* restricted_username, size_t restricted_username_len,
    const char* restricted_password, size_t restricted_password_len,
    const char* add_username, size_t add_username_len,
    const char* add_password, size_t add_password_len,
    int form_auth, int secure_control,
    const char* secure_cert, size_t secure_cert_len,
    const char* secure_key, size_t secure_key_len,
    int secure_port, int cert_check, int cert_store_path_empty,
    int daemon_mode, const char* daemon_username, size_t daemon_username_len,
    int umask, void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_news_server_section(
    int active, const char* name, size_t name_len,
    int level, int optional, int group,
    const char* host, size_t host_len, int port,
    const char* username, size_t username_len,
    const char* password, size_t password_len,
    int tls, const char* cipher, size_t cipher_len,
    int max_connections, int retention, uint32_t cert_level,
    int ip_version, int dupe_search,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_news_servers_group(
    const char* const* host_ptrs, const size_t* host_lens,
    const uint8_t* actives_ptr, const int32_t* max_conns_ptr,
    const int32_t* levels_ptr, size_t count,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);
void nzbget_rs_health_validate_extension_scripts_section(
    const char* extensions_opt, size_t extensions_opt_len,
    const char* const* ext_name_ptrs, const size_t* ext_name_lens,
    const char* const* ext_entry_ptrs, const size_t* ext_entry_lens,
    size_t ext_count,
    const char* shell_override, size_t shell_override_len,
    int event_interval,
    void* ctx, NzbgetRsHealthOptionEmitCb emit_cb);

/* Extension & Script Subsystem End-to-End C ABI (`rust/src/extension.rs`, `rust/src/options.rs`) */
void nzbget_rs_options_convert_old_options(
    const char* const* names, const char* const* values, size_t count,
    void* ctx,
    void (*update_cb)(void* ctx, size_t idx, const char* name, const char* value));

int nzbget_rs_script_config_load(
    const char* filename, void* ctx,
    void (*emit_cb)(void* ctx, const char* name, const char* value));

int nzbget_rs_script_config_save(
    const char* filename,
    const char* const* names, const char* const* values, size_t count,
    const char* cipher_baseline, int* out_conflict);

int nzbget_rs_ext_is_in_list(
    const char* extensions_list, const char* target_name);

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

NzbgetRsBuf nzbget_rs_ext_mgr_install_error(
    const char* file_path, const char* os_err, int kind);

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

void nzbget_rs_feed_info_init(
    int id, const char* name, const char* filter,
    NzbgetRsBuf* out_default_name, uint32_t* out_filter_hash);
int nzbget_rs_feed_parse_prefixed_int(const char* value);
int nzbget_rs_feed_has_any_series_id(
    const char* series, const char* rage_id, const char* tvdb_id, const char* tvmaze_id,
    int item_rage_id, int item_tvdb_id, int item_tvmaze_id);
NzbgetRsBuf nzbget_rs_feed_build_dupe_key(
    const char* rage_id, const char* tvdb_id, const char* tvmaze_id, const char* series,
    int item_rage_id, int item_tvdb_id, int item_tvmaze_id, int item_imdb_id,
    int season_num, int episode_num, const char* season, const char* episode);
void nzbget_rs_feed_format_season_episode(
    int season_num, const char* ep_raw, size_t ep_raw_len,
    NzbgetRsBuf* out_season, NzbgetRsBuf* out_episode);

NzbgetRsFeedFileParser* nzbget_rs_feed_file_parser_parse_file(const char* filename);
void nzbget_rs_feed_file_parser_free(NzbgetRsFeedFileParser* parser);
size_t nzbget_rs_feed_file_parser_finalize(NzbgetRsFeedFileParser* parser);
int nzbget_rs_feed_file_parser_get_item(
    const NzbgetRsFeedFileParser* parser, size_t index,
    NzbgetRsParsedFeedItemView* out_view);

void nzbget_rs_feed_create_nzb_info(
    int feed_id, const char* feed_cat, int category_source,
    const char* item_url, const char* item_filename,
    const char* item_cat, const char* item_add_cat,
    int64_t item_size, int64_t item_time, int item_priority,
    int item_pause_nzb, const char* item_dupe_key,
    int item_dupe_score, int item_dupe_mode,
    NzbgetRsBuf* out_filename, NzbgetRsBuf* out_category);
const char* nzbget_rs_feed_resolve_category(
    const char* feed_cat, int category_source, const char* item_cat);
NzbgetRsBuf nzbget_rs_feed_format_dupe_status_mask(int dupe_status);
int nzbget_rs_feed_should_start(
    int interval_min, int64_t next_update, int64_t current_time, int fetch,
    int status, int fs_running, int* out_set_force);
int nzbget_rs_feed_calc_next_interval(
    int success, int interval_min, int last_interval_sec, int* out_new_last_interval_sec);
NzbgetRsBuf nzbget_rs_feed_format_temp_filename(
    const char* temp_dir, char sep, int id, int preview,
    int cur_time, int rand_val, int preview_num);
int nzbget_rs_feed_classify_item_history(
    int first_fetch, int backlog, int has_history, int* out_should_add);
int nzbget_rs_feed_resolve_preview_item_status(
    int has_history, int history_status, int first_fetch, int backlog);
int64_t nzbget_rs_feed_calc_history_border_date(
    int64_t cur_time, const int64_t* last_updates, size_t last_updates_len,
    int feed_history_days);
int nzbget_rs_feed_should_delete_history_entry(
    int64_t last_seen, int64_t border_date, const char* url,
    size_t* out_url_len_without_query);
int nzbget_rs_feed_is_cache_expired(
    int64_t last_usage, int cache_time_sec, int64_t cur_time);

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
void nzbget_rs_nzbfile_parser_start_element(
    NzbgetRsNzbFileParser* parser, const char* name, const char* const* atts,
    void* alloc_ctx, NzbgetRsAllocFileIdCb alloc_file_id_cb);
void nzbget_rs_nzbfile_parser_end_element(
    NzbgetRsNzbFileParser* parser, const char* name);
void nzbget_rs_nzbfile_parser_characters(
    NzbgetRsNzbFileParser* parser, const char* xmlstr, int len);
void nzbget_rs_nzbfile_parser_on_missing_entity(
    NzbgetRsNzbFileParser* parser);
void nzbget_rs_nzbfile_parser_on_sax_error(
    NzbgetRsNzbFileParser* parser, const char* err_msg);
int nzbget_rs_nzbfile_parser_finalize(
    NzbgetRsNzbFileParser* parser, int sax_ret,
    const char* par_ignore_ext, const NzbgetRsCtype* ct,
    void (*on_parsed_subject)(const char*, const char*),
    void (*sort_indices)(const char* const*, size_t*, size_t),
    NzbgetRsNzbFileSummary* out_summary);
int nzbget_rs_nzbfile_parser_get_message(
    const NzbgetRsNzbFileParser* parser, size_t index,
    int* out_kind, const char** out_text);
int nzbget_rs_nzbfile_parser_get_file(
    const NzbgetRsNzbFileParser* parser, size_t index,
    NzbgetRsNzbKeptFileView* out_view);

int nzbget_rs_url_coord_select_next_url(
    const int* kinds, const int* url_statuses, const int* delete_statuses,
    const int* priorities, size_t count, int nk_url, int ls_none, int ds_none);
int nzbget_rs_url_coord_is_hanging_download(
    int64_t now, int64_t last_update_time, int url_timeout, int status, int ad_running);
NzbgetRsBuf nzbget_rs_url_coord_format_temp_path(
    const char* temp_dir, char sep, int32_t id);

void nzbget_rs_direct_renamer_eval_article(
    int article_size, size_t article_count, int old_par_file, int new_par_file,
    int file_paused, int64_t file_size, int64_t file_success_size,
    int64_t file_failed_size, int64_t file_missed_size,
    int64_t par_size, int64_t par_current_success_size,
    int64_t par_current_failed_size, int64_t par_failed_size,
    int remaining_par_count, NzbgetRsDirectRenameParDelta* out_delta);
size_t nzbget_rs_direct_renamer_select_pars_to_unpause(
    const int* ids, const char* const* set_ids, const uint8_t* completed,
    const uint8_t* in_file_list, const int64_t* sizes, size_t count,
    int* out_file_ids);
NzbgetRsBuf nzbget_rs_direct_renamer_resolve_old_output_path(
    const char* curr_output_filename, const char* dest_dir, char sep, const char* filename);

int nzbget_rs_downloadinfo_script_calc_total_status(const int32_t* statuses, size_t len);
void nzbget_rs_downloadinfo_server_stat_apply_op(
    int cur_success, int cur_failed, int delta_success, int delta_failed,
    int stat_op, int so_set, int so_add, int so_subtract,
    int* out_success, int* out_failed);
NzbgetRsBuf nzbget_rs_downloadinfo_make_nice_nzb_name(const char* nzb_filename, int remove_ext);
NzbgetRsBuf nzbget_rs_downloadinfo_make_nice_url_name(
    const char* url_str, const char* nzb_filename);
NzbgetRsBuf nzbget_rs_downloadinfo_build_dest_dir_name(
    const char* inter_dir, const char* default_dest_dir, const char* category_dest_dir,
    const char* category, int append_category_dir, const char* name, int nzb_id);
NzbgetRsBuf nzbget_rs_downloadinfo_build_final_dir_name(
    const char* default_dest_dir, const char* category_dest_dir,
    const char* category, int append_category_dir, const char* name, int nzb_id);
void nzbget_rs_downloadinfo_update_min_max_time(
    const int64_t* times, size_t len, int64_t* out_min, int64_t* out_max);
void nzbget_rs_downloadinfo_calc_active_downloads_sec(
    int old_active, int new_active, int is_nzb_kind, int64_t cur_time,
    int download_sec, int64_t download_start_time, int download_start_sec,
    int* out_sec, int64_t* out_start_time, int* out_start_sec, int* out_changed);
int nzbget_rs_downloadinfo_is_dupe_success(
    int mark_status, int delete_status, int par_status, int unpack_status,
    int url_status, int health, int critical_health_est, int critical_health_exact,
    int health_check_dupe);
const char* nzbget_rs_downloadinfo_make_text_status(
    int kind, int mark_status, int delete_status, int par_status, int unpack_status,
    int move_status, int url_status, int script_status, int health, int critical_health);
void nzbget_rs_downloadinfo_update_current_stats_step(
    NzbgetRsDpsCurrentStatsAcc* acc, int64_t file_remaining_size,
    int file_success_articles, int file_failed_articles,
    int64_t file_success_size, int64_t file_failed_size,
    int file_extra_priority, int file_paused, int file_par_file);
void nzbget_rs_downloadinfo_update_completed_stats(
    int64_t file_success_size, int64_t file_failed_size,
    int file_failed_articles, int file_success_articles,
    int file_extra_priority, int file_par_file, int file_paused,
    int64_t* inout_success_size, int64_t* inout_failed_size,
    int* inout_failed_articles, int* inout_success_articles,
    int* inout_extra_priority, int64_t* inout_par_success_size,
    int64_t* inout_par_failed_size, int* inout_remaining_par_count,
    int* inout_paused_file_count);
void nzbget_rs_downloadinfo_update_deleted_stats(
    int64_t file_size, int64_t file_success_size, int64_t file_failed_size,
    int64_t file_missed_size, int file_total_articles, int file_success_articles,
    int file_failed_articles, int file_missed_articles, int64_t file_remaining_size,
    int file_extra_priority, int file_par_file, int file_paused,
    int* inout_file_count, int64_t* inout_size, int64_t* inout_cur_success_size,
    int64_t* inout_failed_size, int* inout_failed_articles, int64_t* inout_cur_failed_size,
    int* inout_total_articles, int* inout_cur_success_articles, int* inout_cur_failed_articles,
    int64_t* inout_remaining_size, int* inout_extra_priority, int* inout_remaining_par_count,
    int64_t* inout_par_size, int64_t* inout_par_cur_success_size,
    int64_t* inout_par_failed_size, int64_t* inout_par_cur_failed_size,
    int* inout_paused_file_count, int64_t* inout_paused_size);
int nzbget_rs_downloadinfo_completed_same_filename(const char* a, const char* b);

size_t nzbget_rs_dupe_probe_select_server_order(
    const uint8_t* active, const int32_t* groups, const int32_t* norm_levels,
    const int32_t* ids, size_t count, int32_t* out_order, size_t out_cap);
NzbgetRsBuf nzbget_rs_dupe_probe_eval_run_outcome(
    int existing, int sampled, int missing_servers, int active_servers,
    int unreachable_servers, int reached_servers, int finished,
    const char* found_on, int* out_outcome);
NzbgetRsBuf nzbget_rs_dupe_probe_eval_recheck_outcome(
    int existing, int sampled, int reached_servers,
    const char* duplicate_queued_name, int* out_outcome);

int nzbget_rs_dupecoord_files_on_disk(
    const char* final_dir, const char* dest_dir, const char* ext_cleanup_disk);
NzbgetRsBuf nzbget_rs_dupecoord_format_dupe_skip_message(
    const char* name, const char* other_name, int in_history, int same_content, int good);
NzbgetRsBuf nzbget_rs_dupecoord_no_backup_reason(
    int fails_over_flag, int dupe_mode, const NzbgetRsDupeHistoryCandidate* history,
    size_t history_len, void* cb_ctx, int (*file_exists_cb)(void* ctx, size_t history_index));

int nzbget_rs_unpack_use_inter_dir(const char* dest_dir, const char* inter_dir, char sep);
int nzbget_rs_unpack_is_main_archive(const char* filename);
int nzbget_rs_unpack_direct_readline_step(
    uint8_t* buf, size_t buf_len, size_t i, int ch, int* backspace, size_t* out_idx);
int nzbget_rs_unpack_classify_archive_file(const char* filename);
int nzbget_rs_unpack_has_completed_archive_ext(const char* filename);
int nzbget_rs_unpack_is_split_first_piece(const char* filename);
int nzbget_rs_unpack_accumulate_split_piece(
    const char* filename, const char* dest_base_name, int64_t segment_size,
    int64_t first_segment_size, int* inout_count, int* inout_min, int* inout_max,
    int* inout_dif_size_min);
int nzbget_rs_unpack_classify_cleanup_archive_file(
    int is_dir, int inter_dir, int in_extracted_files, int is_downloaded_file,
    int has_splitted_files, int in_joined_files, const char* filename);
int32_t nzbget_rs_unpack_ctrl_readline_step(
    uint8_t* buf, size_t buf_len, size_t* idx, int ch, int32_t* printed,
    ptrdiff_t* last_bs, ptrdiff_t* out_process_len, int32_t* out_progress);

int nzbget_rs_scanner_should_skip_item(
    const char* filename, const char* processed_dir, const char* broken_dir);
int nzbget_rs_scanner_classify_incoming_extension(
    int ran_scan_script, int exists, const char* extension, int* out_rename_to_processed);
int nzbget_rs_scanner_resolve_archive_add_status(
    int has_url_info, const int* statuses, size_t len, int as_success, int as_skipped);
int nzbget_rs_scanner_is_absolute_nzb_name(const char* nzb_name);

int nzbget_rs_queue_editor_group_sorter_compare_values(
    int sort_criteria, int sort_order, int so_descending, int cmp_name,
    int64_t size1, int64_t size2, int64_t rem1, int64_t rem2,
    int64_t min_time1, int64_t min_time2, int cmp_cat, int prio1, int prio2);
int nzbget_rs_queue_editor_clamp_move_target(
    int entry, int offset, int size, int* out_new_entry);
size_t nzbget_rs_queue_editor_dedup_id_list(
    const int32_t* ids_ptr, size_t len, int32_t* out_ids_ptr);
void nzbget_rs_queue_editor_eval_group_delete_flags(
    int action, int ea_park_delete, int ea_dupe_delete, int ea_final_delete,
    int keep_history, int unpack_cleaned_up_disk, int current_success_articles,
    int* out_parking, int* out_avoid_history, int* out_set_dupe_status, int* out_cleanup_disk);
int32_t nzbget_rs_queue_editor_map_group_to_file_action(int32_t action);
int nzbget_rs_queue_editor_filename_has_vol(const char* filename);
void nzbget_rs_queue_editor_visit_category_ext_diff(
    const char* old_ext, const char* new_ext, void* ctx,
    void (*cb)(void* ctx, const char* param_name, const char* param_val));
void nzbget_rs_queue_editor_resolve_move_groups_target(
    int32_t target_id, const int32_t* queue_ids_ptr, size_t queue_len,
    const int32_t* moved_ids_ptr, size_t moved_len, int32_t before, int32_t max_id,
    int32_t* out_target_id, int32_t* out_before, int32_t* out_offset);
int nzbget_rs_queue_editor_calc_move_groups_to_offset(
    const int* queue_ids, size_t queue_len, int target_id, int move_id,
    int before, size_t item_list_size);

int64_t nzbget_rs_history_coord_calc_min_time(int64_t cur_time, int keep_history_days);
int nzbget_rs_history_coord_classify_service_work_item(
    int kind, int hk_dup, int hk_nzb, int64_t item_time, int64_t min_time, int dupe_check);
int nzbget_rs_history_coord_should_recheck_failed_articles(
    int daf_none, int hc_dupe, int kind_nzb, int failed_articles,
    const char* text_status, int has_dupe_recheck_param);
int nzbget_rs_history_coord_should_delete_dest_dir(
    int is_nzb, int delete_status, int ds_none, int par_status, int ps_failure,
    int unpack_status, int us_failure, int us_password, int dir_exists);
void nzbget_rs_history_coord_calc_move_to_queue_reset(
    int unpack_cleaned_up_disk, int post_total_sec, int unpack_sec, int has_main_pars,
    int par_sec, int move_status, int ms_failure, int ms_none,
    int* out_new_post_total_sec, int* out_new_move_status);
void nzbget_rs_history_coord_retry_action_labels(
    int reset_failed, int reprocess, const char** out_wrong_type_verb,
    const char** out_missing_dir_verb, const char** out_start_msg_verb);
int nzbget_rs_history_coord_eval_requeue_loaded_file(
    int status, int cf_none, int cf_failure, int cf_partial,
    int reset_failed, int state_loaded, int64_t remaining_size);
int nzbget_rs_history_coord_has_retryable_article(
    const int* statuses, size_t len, int reset_failed, int ai_undefined, int ai_failed);
void nzbget_rs_history_coord_reset_articles_header(
    int all_failed, int is_par_file, int64_t file_size, int64_t file_missed_size,
    int file_total_articles, int file_missed_articles, int64_t cur_file_failed_size,
    int cur_file_failed_articles, int64_t cur_file_remaining_size,
    int cur_file_completed_articles, int64_t file_success_size, int file_success_articles,
    int64_t cur_nzb_failed_size, int64_t cur_nzb_success_size,
    int cur_nzb_failed_articles, int cur_nzb_success_articles,
    int64_t cur_nzb_par_failed_size, int64_t cur_nzb_par_success_size,
    int64_t* out_file_failed_size, int* out_file_failed_articles,
    int64_t* out_file_remaining_size, int* out_file_completed_articles,
    int64_t* out_nzb_failed_size, int64_t* out_nzb_success_size,
    int* out_nzb_failed_articles, int* out_nzb_success_articles,
    int64_t* out_nzb_par_failed_size, int64_t* out_nzb_par_success_size);
void nzbget_rs_history_coord_apply_article_reset_delta(
    int pa_status, int ai_failed, int ai_undefined, int ai_finished, int pa_size,
    int cur_completed_articles, int64_t cur_remaining_size, int cur_failed_articles,
    int64_t cur_failed_size, int cur_success_articles, int64_t cur_success_size,
    int* out_completed_articles, int64_t* out_remaining_size, int* out_failed_articles,
    int64_t* out_failed_size, int* out_success_articles, int64_t* out_success_size);
int nzbget_rs_history_coord_parse_dupe_backup_flag(
    const char* text, int ds_dupe, int ds_manual);

int nzbget_rs_repair_filename_eq_ignore_case(const char* a, const char* b);
int nzbget_rs_repair_should_load_partial_segments(
    int status, int cf_partial, int id, int reprocess);
int nzbget_rs_repair_classify_file_crc_status(
    int cf_status, int reprocess, int has_segments,
    int cf_success, int cf_failure, int cf_partial,
    int fs_success, int fs_failure, int fs_partial, int fs_unknown);
NzbgetRsBuf nzbget_rs_repair_build_twin_filename(
    const char* entry_filename, size_t twin_files_count);
int nzbget_rs_repair_should_consider_twin_candidate(
    int is_self, int kind, int nk_nzb, int same_name_or_key,
    const char* queued_filename, int has_print, int own_print_matches);
ptrdiff_t nzbget_rs_repair_select_best_twin_par2_index(
    const uint8_t* is_par2_ptr, const int64_t* sizes_ptr, size_t len, int64_t max_index_size);
const char* nzbget_rs_repair_twin_par2_reject_reason(int cmp);
int nzbget_rs_repair_should_skip_twin_volume_entry(
    int is_par2, const char* entry_filename, const char* twin_index);
int nzbget_rs_repair_estimate_twin_vol_blocks(int64_t entry_size, uint64_t twin_block_size);
int nzbget_rs_repair_sum_and_check_twin_blocks(
    const int* vol_blocks_ptr, size_t vol_blocks_len, int block_needed, int* out_available);
int nzbget_rs_repair_should_use_par_quick(
    int par_quick_opt, int force_par_full, int dupe_repaired);
void nzbget_rs_repair_complete_par_check(
    int checker_status, int cur_par_status,
    int ps_repaired, int ps_repair_not_needed, int ps_repair_possible,
    int nzb_ps_skipped, int nzb_ps_success, int nzb_ps_repair_possible, int nzb_ps_failure,
    int cur_dl_sec, int start_dl_sec, int64_t cur_time, int64_t par_time,
    int* out_par_status, int* out_set_par_repaired, int* out_par_repaired_val,
    int* out_wait_time, int* out_par_sec);
int nzbget_rs_repair_eval_find_pars_candidate(
    int same_collection, int deleted, int strict_par_name,
    const char* filename, const char* main_base,
    int parsed_ok, int parsed_blocks, int64_t file_size, int64_t block_size,
    int* out_blocks);
int nzbget_rs_repair_match_strict_par_name(const char* filename, const char* main_base);
int nzbget_rs_repair_eval_par_progress_update(
    int cur_post_stage, int checker_stage,
    int pt_loading_pars, int pt_verifying_sources, int pt_repairing, int pt_verifying_repaired,
    int64_t current_time, int64_t repair_time, int cur_repair_sec,
    int* out_new_post_stage, int* out_set_repair_time, int* out_new_repair_sec);
void nzbget_rs_repair_calc_paused_timestamps(
    int64_t cur_time, int64_t wait_time,
    int64_t stage_time, int64_t start_time, int64_t par_time, int64_t repair_time,
    int64_t* out_stage, int64_t* out_start, int64_t* out_par, int64_t* out_repair);

int nzbget_rs_content_map_calc_decrypt_cipher_bounds(
    int64_t inner_offset, int64_t inner_size, int64_t inner_size_total,
    int64_t block_size, int64_t cipher_stream_size,
    int* out_empty, int64_t* out_cipher_from, int64_t* out_cipher_to);
int nzbget_rs_content_mapper_is_compressible_archive(
    int format, int mf_rar, int mf_zip, int mf_seven_zip);
int nzbget_rs_content_mapper_classify_unreadable_rar_vol(
    int has_source, int is_encrypted, int unknown_volume);

ptrdiff_t nzbget_rs_twin_select_min_par2_index(
    const uint8_t* is_par2_ptr, const int64_t* sizes_ptr, size_t len, int64_t max_index_size);
int nzbget_rs_twin_parse_fingerprint_files(const char* fingerprint);
int nzbget_rs_twin_compare_entry_names(
    const char* a_filename, const char* a_subject,
    const char* b_filename, const char* b_subject);
int nzbget_rs_twin_sample_indices(
    const size_t* primary_seg_counts, const int64_t* primary_seg_bytes, size_t primary_seg_bytes_len,
    size_t primary_files, const size_t* dupe_seg_counts, const int64_t* dupe_seg_bytes,
    size_t dupe_seg_bytes_len, size_t dupe_files, size_t sample_count,
    size_t* out_file_indices, size_t* out_seg_indices);
void nzbget_rs_twin_compare_sampled_pair(
    int part_a_success, int part_b_success, int64_t part_a_offset,
    int64_t part_b_offset, int data_equal, int in_compared, int in_differ,
    int* out_compared, int* out_differ);
int nzbget_rs_twin_eval_sample_verdict(
    int is_stopped, int compared, int differ, int sample_count,
    const char** out_label);
NzbgetRsBuf nzbget_rs_twin_format_identity(const char* files_param, const char* posting);
NzbgetRsBuf nzbget_rs_twin_split_put_par2_result(const char* result_str, int* out_complete);
int nzbget_rs_twin_is_fetched_entry_part_valid(
    int success, int64_t offset, size_t data_len, int64_t max_size,
    size_t cur_data_len, size_t* out_new_data_len);
int nzbget_rs_twin_is_dupe_item(
    int queued, int64_t remaining_size, int64_t paused_size,
    int delete_status, int ds_dupe, int ds_copy);
int nzbget_rs_twin_is_print_usable(const char* print);
int nzbget_rs_twin_select_primary(
    const uint8_t* queued, const uint8_t* is_dupe, const int* scores, size_t count);
int nzbget_rs_twin_eval_sampleable(
    int primary_ok, int is_primary, int is_dupe, int print_empty,
    int primary_print_empty, int by_par2, int has_sampled,
    const char* sampled_of, const char* primary_id, int* out_sampled_now);
NzbgetRsBuf nzbget_rs_twin_resolve_kind(
    int is_primary, int is_dupe, int by_par2, const char* by_files, int listed, int same_print,
    int sampleable, int sampled_now, const char* sampled_val);

int nzbget_rs_streamcrypto_derive_rar3(
    const char* password, const uint8_t* salt8, uint8_t* key16_out, uint8_t* iv16_out);
int nzbget_rs_streamcrypto_derive_rar5(
    const char* password, uint8_t kdf_count, const uint8_t* salt16, uint8_t* key32_out);
int nzbget_rs_streamcrypto_derive_rar5_psw_check(
    const char* password, uint8_t kdf_count, const uint8_t* salt16, uint8_t* check8_out);
int nzbget_rs_streamcrypto_crypt_cbc(
    int encrypt, const uint8_t* key, size_t key_len, const uint8_t* iv16,
    const uint8_t* in_buf, uint8_t* out_buf, int64_t blocks);

int nzbget_rs_stream_eval_build_repair_job(
    int fallback_mode, int daf_stream, int raw_article, int is_par_file,
    const char* disk_basename, int has_nzb, int delete_status, int ds_none,
    int ds_health, int deleting, int parking, int has_post_info,
    int articles_empty, int success_articles, int64_t encoded_size,
    int64_t decoded_file_size);
int nzbget_rs_stream_donor_check_fetched_part(
    int success, size_t data_len, int64_t offset, int64_t fetched_file_size,
    int64_t member_size, int* out_mark_member_bad);
int nzbget_rs_stream_donor_index_for_offset(
    int64_t offset, int64_t drift,
    const NzbgetRsByteRange* ranges, size_t ranges_len);
int nzbget_rs_stream_donor_step_part_for_offset(
    int64_t offset, int64_t fetched_offset, size_t fetched_len,
    int index, size_t ranges_len, int* out_new_index);
void nzbget_rs_stream_donor_calc_prefetch_bounds(
    int first_est, int last_est, size_t ranges_len,
    int* out_first, int* out_last);
void nzbget_rs_stream_donor_calc_read_chunk(
    int64_t pos, int64_t fetched_offset, size_t fetched_data_len,
    int64_t end_offset, int64_t* out_from, int64_t* out_len);
int nzbget_rs_stream_watchdog_eval_tick(
    int64_t progress_bytes, int64_t prev_progress, int waiting, int materializing,
    int64_t since_last_progress_sec, int64_t since_begun_sec, int64_t since_start_sec,
    double elapsed_sec, int timeout_sec, int max_run_factor, int max_conn_wait_sec,
    int slow_repair_factor, int64_t download_rate, int64_t start_progress,
    int64_t missing_bytes, int64_t* out_new_progress, int* out_reset_last_progress,
    NzbgetRsBuf* out_reason);
NzbgetRsBuf nzbget_rs_stream_format_whole_files_suffix(int unsized_count);
int nzbget_rs_stream_is_par2_job_or_file(int is_par_file, const char* filename);
int nzbget_rs_stream_eval_donor_outcome_bail(
    int outcome, int ro_productive, int ro_unproductive, int donor_proven,
    int cur_consecutive_failures, int donor_failure_bail, int unproven_donor_bail,
    int* out_new_consecutive_failures);
size_t nzbget_rs_stream_select_probe_replacements(
    int missing, const int* reserve, size_t reserve_len, int max_probe_fetches,
    size_t attempted_len, int* out_batch, size_t max_out);
int nzbget_rs_stream_check_probe_article(
    int success, size_t data_len, int64_t file_size, int64_t offset,
    int64_t target_decoded_size, int* out_is_missing);
int nzbget_rs_stream_is_patch_article_usable(
    int success, int64_t fetched_file_size, int64_t target_decoded_size,
    size_t data_len, int64_t fetched_offset);
int nzbget_rs_stream_clip_hole_to_fetched(
    int64_t hole_offset, int64_t hole_size, int64_t fetched_offset, size_t fetched_len,
    int64_t* out_from, int64_t* out_to);
int nzbget_rs_stream_classify_remaining_hole_target(
    int interrupted, int64_t decoded_file_size, int holes_empty, int is_par_file,
    int failed_articles, int64_t failed_size, int64_t missed_size,
    int* inout_recovered_articles, int64_t* inout_recovered_failed_size,
    int64_t* inout_recovered_failed_par_size);
int nzbget_rs_stream_is_incomplete_nzb_hole(int64_t missing, int64_t encoded_size);
int nzbget_rs_stream_patch_donor_set_step_failure(
    int64_t chunk, int64_t min_chunk, int64_t* inout_size, int64_t* inout_pos,
    int* inout_failed_pieces, int max_failed_pieces);
int nzbget_rs_stream_calc_encrypted_window_bounds(
    int64_t window_offset, int64_t window_end_raw, int64_t last_computable,
    int64_t block_from, int64_t block_size,
    int64_t* out_window_end, int64_t* out_from, int64_t* out_block_to);
int nzbget_rs_stream_encrypted_clip_live_hole(
    int64_t hole_offset, int64_t hole_size,
    const NzbgetRsByteRange* live_holes, size_t live_holes_len,
    int64_t last_computable, int64_t block_size,
    int64_t* inout_uncomputable_tail, int64_t* out_hole_from,
    int64_t* out_patch_to, int64_t* out_block_from, int64_t* out_block_to);
int nzbget_rs_stream_encrypted_is_prev_block_present(
    int64_t block_from, int64_t block_size,
    const NzbgetRsByteRange* live_holes, size_t live_holes_len);
void nzbget_rs_stream_calc_encrypted_chunk_end(
    int64_t block_from, int64_t block_to, int64_t max_chunk, int64_t block_size,
    int64_t* out_chunk_end, int64_t* out_block_count);
NzbgetRsBuf nzbget_rs_stream_format_materialize_progress(
    const char* donor_name, int member_num, int total_members,
    int delivered_articles, int total_articles, int64_t total_bytes);
int nzbget_rs_stream_materialize_article_valid(
    int success, size_t data_len, int64_t offset, int64_t fetched_file_size,
    int64_t member_size);
void nzbget_rs_stream_credit_repaired_sizes(
    int64_t recovered_failed_size, int64_t recovered_failed_par_size,
    int64_t* inout_failed_size, int64_t* inout_success_size,
    int64_t* inout_current_failed_size, int64_t* inout_current_success_size,
    int64_t* inout_par_failed_size, int64_t* inout_par_success_size,
    int64_t* inout_par_current_failed_size, int64_t* inout_par_current_success_size);
void nzbget_rs_stream_resolve_completion_par_action(
    int64_t recovered_bytes, int holes_remain, int par_rename_status, int rs_none,
    int par_status, int ps_skipped, int par_check_opt, int pc_manual,
    int ps_manual, int ps_none, int* out_reset_par_rename,
    int* out_update_par_status, int* out_new_par_status,
    int* out_log_reenabling, int* out_request_par_check,
    int* out_log_remaining_holes);

int nzbget_rs_qhp_rename_completed_file(
    size_t count, const char* old_name, const char* new_name, void* ctx,
    void (*get_cb)(void* ctx, size_t idx, const char** out_filename, const char** out_origname),
    void (*set_cb)(void* ctx, size_t idx, const char* new_filename, const char* new_origname));

int nzbget_rs_dupematcher_prepare(
    const char* dest_dir, int64_t expected_size, int64_t* out_max_size, int* out_compressed,
    void (*on_rar_archive)(void* ctx, char* filename_buf, int buf_len, int64_t expected_size, int64_t* out_max_size, int* out_compressed),
    void (*on_detail_log)(void* ctx, const char* msg), void* ctx);
int nzbget_rs_dupematcher_match_dupe_content(
    const char* dupe_dir, int64_t max_size, int compressed,
    void (*on_rar_archive)(void* ctx, char* filename_buf, int buf_len, int64_t expected_size, int64_t* out_max_size, int* out_compressed),
    void (*on_detail_log)(void* ctx, const char* msg), void* ctx);

int nzbget_rs_archive_processor_process(
    const char* archive_file, const char* unpack_dir, const char* dest_dir,
    const char* processed_dir, const char* broken_dir, int config_action, void* ctx,
    int (*extract_cb)(void* ctx, const char* archive_file, const char* unpack_dir, int* out_extracted),
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*moved_cb)(void* ctx, const char* moved_path));

int64_t nzbget_rs_donor_now_ms(void);
void nzbget_rs_donor_stop_all(void);
void nzbget_rs_donor_wait_all(void);
void nzbget_rs_donor_reset(void);
int nzbget_rs_donor_stopping(void);
void nzbget_rs_donor_sample(
    const char* const* ids_ptr, const size_t* ids_lens, size_t ids_count,
    int percent, int minimum, int maximum, uint32_t seed, void* ctx,
    void (*emit_cb)(void* ctx, const char* id_ptr, size_t id_len));
void nzbget_rs_donor_plan(
    const char* const* ids_ptr, const size_t* ids_lens, size_t ids_count,
    int percent, int minimum, int maximum, int body_percent, int max_body,
    uint32_t seed, void* ctx,
    void (*emit_cb)(void* ctx, const char* id_ptr, size_t id_len, int body));
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
int nzbget_rs_donor_server_depth(const void* handle);
int nzbget_rs_donor_server_paused(const void* handle);
int nzbget_rs_donor_server_max_active(const void* handle);
void nzbget_rs_donor_server_set_retry_after_ms(const void* handle, int ms);
void nzbget_rs_donor_server_ask(
    const void* handle, size_t count, const char* const* msg_ids, const size_t* msg_lens,
    void* const* rust_gates, const char* const* const* group_ptrs,
    const size_t* const* group_lens, const size_t* group_counts,
    void* cancel_ctx, int (*cancel_cb)(void* cancel_ctx), int* out_answers);
void nzbget_rs_donor_check_items(
    void* const* server_handles, size_t server_count,
    void* keepalive_ptr, void (*keepalive_drop)(void* keepalive_ptr),
    const char* const* msg_ids, const size_t* msg_lens, const int* bodies,
    const char* const* const* group_ptrs, const size_t* const* group_lens,
    const size_t* group_counts, size_t item_count, int budget_ms,
    int* out_checked, int* out_present, int* out_missing, int* out_error,
    int* out_body_checked, int* out_body_bad);
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
int nzbget_rs_nzb_fetcher_is_refused(void* handle, const char* indexer, size_t indexer_len, int64_t now);
int nzbget_rs_nzb_fetcher_refuse(void* handle, const char* indexer, size_t indexer_len, int64_t now);
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

int nzbget_rs_downloadinfo_is_download_completed(
    int active_downloads, int ignore_paused_pars, size_t file_count, void* ctx,
    void (*get_file_cb)(
        void* ctx, size_t idx, int* out_paused, int* out_par_file, int* out_deleted));

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

#include "nzbget_tls_cipher.h"

#ifdef __cplusplus
}
#endif

#endif
