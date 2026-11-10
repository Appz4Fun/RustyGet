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

// raw may be NULL (treated as empty); otherwise it must point to a readable
// NUL-terminated string for the duration of the call. Results own their storage.
// Allocation failure or a Rust panic aborts; unwinding never crosses this ABI.
NzbgetRsBuf nzbget_rs_json_encode(const char* raw);
NzbgetRsBuf nzbget_rs_xml_encode(const char* raw);
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

// RPC request decoders (rust/src/decode.rs), as WebUtil's:
// All buffers remain caller-owned; these functions allocate no result buffers.
// Base64: NULL input returns 0. Otherwise input is readable for length bytes,
// or, when length <= 0, through the NUL (strlen is truncated to uint32 as in
// C++). Output has room for len / 4 * 3 bytes, and is input or disjoint from it.
// Returns bytes written, without a terminator.
unsigned int nzbget_rs_decode_base64(const char* input, int length, char* output);
// raw is NULL (no-op) or a writable NUL-terminated string, decoded in place.
void nzbget_rs_json_decode(char* raw);

// WebUtil's text helpers (rust/src/text.rs): the in-place ones take NULL (no-op)
// or a caller-owned writable NUL-terminated string. Panics abort.
void nzbget_rs_xml_decode(char* raw);
// NULL input means empty. Results are Rust-owned NUL-terminated buffers:
// copy before freeing with nzbget_rs_free, never with the C allocator.
NzbgetRsBuf nzbget_rs_url_encode(const char* raw);

// WebUtil's finders (rust/src/webutil.rs): a pointer into the text and the
// value length, or NULL with valueLength untouched
const char* nzbget_rs_xml_find_tag(const char* xml, const char* tag, int* valueLength);
// WebUtil::ParseContentDispositionFilename: data is NULL for no file name.
NzbgetRsBuf nzbget_rs_content_disposition_filename(const char* contentDisposition);

// Util's string helpers (rust/src/util.rs). Format results are Rust-owned;
// copy and release with nzbget_rs_free. Numeric formatting uses LC_NUMERIC.
// Panics abort and never unwind through C++.
NzbgetRsBuf nzbget_rs_format_size(long long size);
// Caller-owned writable NUL-terminated str; from/to are NUL-terminated and
// may point into str. Any NULL argument is a no-op. Empty patterns and growing
// replacements are no-ops; equal-length replacements retain C++ truncation.
void nzbget_rs_reduce_str(char* str, const char* from, const char* to);
// NULL strings are empty; non-NULL strings must be readable and NUL-terminated.
int nzbget_rs_match_file_ext(const char* filename, const char* extensionList, const char* listSeparator);

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
void nzbget_rs_error_message(int errnum, char* buf);
// NULL paths return 0.
int nzbget_rs_file_exists(const char* path);
int nzbget_rs_directory_exists(const char* path);
// 1 on success, 0 on failure; every non-NULL errmsg is initialized (empty on
// success or invalid NULL arguments). errmsg must not overlap the input.
int nzbget_rs_force_directories(const char* path, NzbgetRsBuf* errmsg);
// NULL dir/basename mean empty strings.
NzbgetRsBuf nzbget_rs_make_unique_filename(const char* dir, const char* basename);

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

// Limits from this build of Boost.JSON, in decoded bytes / input elements.
typedef struct NzbgetRsJsonLimits
{
	size_t stringSize;
	size_t arraySize;
	size_t objectSize;
} NzbgetRsJsonLimits;

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
int nzbget_rs_delete_directory(const char* dir, char separator);
int nzbget_rs_delete_directory_with_content(const char* dir, char separator, NzbgetRsBuf* errmsg);
int nzbget_rs_rename_bak(const char* filename, const char* bakPart, int removeOldExtension,
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
typedef struct NzbgetRsDonorEntry
{
	int score;
	double alive;
	int twin;
	int64_t bytes;
	int grabs;
} NzbgetRsDonorEntry;
int nzbget_rs_donorscore_base(int pickScore);
void nzbget_rs_donorscore_rerank(const NzbgetRsDonorEntry* entries, size_t n, int64_t pickBytes, int* out);

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
int nzbget_rs_dupe_same_data(uint64_t aBytes, uint32_t aFiles, uint64_t bBytes, uint32_t bFiles);
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
int nzbget_rs_dupe_should_expand(double alive, int32_t missing, int failed, int wholeBackup);
// An opaque per-search budget (3 * maxDonors; zero means unlimited).
// All concurrent callers must have joined before free.
void* nzbget_rs_dupe_budget_new(uint32_t maxDonors);
void nzbget_rs_dupe_budget_free(void* budget);
int nzbget_rs_dupe_budget_spent(const void* budget);

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
void nzbget_rs_memory_pressure_set_mock_root(const char* mock_root);
void nzbget_rs_memory_pressure_trim_heap(void);

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

int nzbget_rs_main(int argc, char* argv[], char* argp[]);
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
uint64_t nzbget_rs_dupe_fallback_steps_hash(const int* sizes, size_t count);
ptrdiff_t nzbget_rs_dupe_fallback_first_untiled(
    const NzbgetRsArticleGeom* articles, size_t count, int64_t decoded_size);
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

size_t nzbget_rs_stream_compute_holes(
    const NzbgetRsByteRange* covered, size_t count, int64_t file_size,
    NzbgetRsByteRange* out_holes, size_t cap);
int64_t nzbget_rs_stream_total_size(const NzbgetRsByteRange* ranges, size_t count);
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
int nzbget_rs_dupecoord_parse_alive_permille(const char* value);
int nzbget_rs_dupecoord_select_dupe_backup(
    int has_self_nzb, uint32_t self_full_content_hash, int dupe_health,
    const NzbgetRsDupeHistoryCandidate* history, size_t history_count,
    const NzbgetRsDupeQueueCandidate* queue, size_t queue_count,
    void* ctx, int (*file_exists_cb)(void* ctx, size_t history_index));

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

int nzbget_rs_qhp_calc_health(
    int64_t size, int64_t par_size, int64_t current_failed_size, int64_t par_current_failed_size);
int nzbget_rs_qhp_calc_critical_health(
    int64_t size, int64_t par_size, int64_t par_current_failed_size, int allow_estimation);

int nzbget_rs_qhp_article_cache_should_flush(
    size_t allocated, size_t effective_limit, int tier, int just_flushed, int reset_counter,
    int is_stopped, int direct_write, int* out_flush_everything, int* out_need_emergency_flush);
int nzbget_rs_qhp_memory_monitor_step(
    int raw_sample, int current_tier, int* candidate_tier, int* consecutive_samples,
    int* out_next_tier);

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
int nzbget_rs_dsp_parse_log_line(
    const char* line, int* out_time, int* out_kind, size_t* out_text_offset);

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

int nzbget_rs_df_placement_available(
    int has_backup, int is_nzb_kind, int deleting, int delete_status,
    int ds_none, int ds_manual, int ds_dupe, int ds_copy,
    int mark_status, int ks_bad, int dupe_mode, int dm_force,
    int in_history, int whole_backup, int has_post_info, int is_dupe_success,
    int has_pick, uint32_t pick_full_hash, uint32_t backup_full_hash,
    int queued_file_exists, int health, int critical_health,
    int health_check_is_dupe, int has_marked_alive, int marked_alive_val);
void nzbget_rs_df_parse_pending_meta(
    const char* meta, size_t len, NzbgetRsPendingMetaView* out_view);
int nzbget_rs_df_parse_dupesearch_state_line(
    const char* line, size_t len, int64_t now, int64_t state_ttl_sec,
    NzbgetRsDupeSearchStateLine* out_line);


NzbgetRsBuf nzbget_rs_df_build_newznab_url(
    const char* base, const char* const* param_keys, const char* const* param_vals,
    size_t param_count, const char* api_key);
NzbgetRsBuf nzbget_rs_df_mask_newznab_text(const char* text, size_t len);

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================

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

int nzbget_rs_sus_analyse_post_script_exit_code(
    int exit_code, int par_status, int ps_skipped, int parcheck_enabled,
    int sr_none, int sr_failure, int sr_success,
    int* out_message_action, int* out_request_par_check);
int nzbget_rs_sus_parse_post_script_cmd(
    const char* msg_text, NzbgetRsPostScriptCmd* out_cmd);

int nzbget_rs_sus_parse_scan_script_cmd(
    const char* msg_text, NzbgetRsScanScriptCmd* out_cmd);

int nzbget_rs_rco_webdl_check_body_chunk(
    int chunk_len, int content_len, int written_len, int gzip, int confirmed_length);

void nzbget_rs_nzb_generator_execute(
    const char* data_dir, int segment_size,
    void (*log_cb)(int kind, const char* msg));

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
void nzbget_rs_nserv_parse_args(
    int argc,
    const char* const* argv,
    void* ctx,
    void (*set_fields_cb)(
        void* ctx,
        const char* data_dir,
        const char* cache_dir,
        const char* bind_address,
        int first_port,
        int instances,
        const char* log_file,
        const char* secure_cert,
        const char* secure_key,
        int generate_nzb,
        int segment_size,
        int quit,
        int latency,
        int speed,
        int mem_cache,
        int param_error),
    void (*push_opt_cb)(void* ctx, const char* opt));


int nzbget_rs_network_speed_test_run(
    void* ctx,
    int (*execute_iter_cb)(
        void* ctx, const char* req_ptr, size_t req_len, int64_t timeout_sec,
        uint32_t* out_downloaded, double* out_elapsed),
    void (*log_cb)(void* ctx, int kind, const char* msg),
    double* out_speed_mbps,
    NzbgetRsBuf* out_err_msg);

void nzbget_rs_nntp_server_pool_eval_block_server(
    int64_t prev_block_time, int64_t cur_time, int retry_interval,
    int* out_new_block, int* out_should_warn);

int nzbget_rs_awc_article_writer_build_output_filenames(
    const char* tmp_dir, const char* dest_dir, char sep, int file_id, int part_num,
    int direct_write, int force_direct_write, const char* existing_output_filename,
    int output_initialized, const char* file_filename,
    void* cb_ctx, int (*file_exists_cb)(void* ctx, const char* path),
    NzbgetRsBuf* out_result_filename, NzbgetRsBuf* out_temp_filename,
    NzbgetRsBuf* out_output_filename, int* out_has_output_filename,
    int* out_update_file_info_output_filename);
int nzbget_rs_awc_article_writer_execute_setup_output_file(
    const char* dest_dir, size_t dest_dir_len,
    const char* filename, size_t filename_len,
    char sep, int raw_article, int direct_write, int force_direct_write,
    const char* output_filename, size_t output_filename_len,
    void* cb_ctx,
    int (*same_filename_cb)(void* ctx, const char* a, const char* b),
    size_t (*make_unique_cb)(void* ctx, const char* dir, const char* fname, char* out_buf, size_t out_cap),
    int (*file_exists_cb)(void* ctx, const char* path),
    int (*force_dirs_cb)(void* ctx, const char* path),
    int (*delete_file_cb)(void* ctx, const char* path),
    int (*open_file_cb)(void* ctx, const char* path, int read_write),
    int (*create_dir_cb)(void* ctx, const char* path),
    NzbgetRsBuf* out_final_path, NzbgetRsBuf* out_temp_path,
    NzbgetRsBuf* out_updated_output_filename,
    NzbgetRsBuf* out_parent_dir_failed, int* out_has_parent_dir_failed,
    int* out_error_kind, NzbgetRsBuf* out_error_path);
void nzbget_rs_awc_article_writer_execute_cleanup_old_data(
    int direct_write,
    const char* nzb_dest_dir, size_t nzb_dest_dir_len,
    const char* final_output_path, size_t final_output_path_len,
    const char* output_filename, size_t output_filename_len,
    int is_hard_linked, int inter_dir_empty, char sep1, char sep2,
    const int* dupe_fallback_rounds, const char* const* result_filenames, size_t articles_len,
    void* cb_ctx,
    int (*same_filename_cb)(void* ctx, const char* a, const char* b),
    int (*file_exists_cb)(void* ctx, const char* path),
    int (*delete_file_cb)(void* ctx, const char* path),
    int (*move_file_cb)(void* ctx, const char* from, const char* to),
    int (*dir_empty_cb)(void* ctx, const char* dir),
    int (*remove_dir_cb)(void* ctx, const char* dir),
    int* out_move_failed,
    NzbgetRsBuf* out_checked_old_dest_dir, int* out_has_checked_old_dest_dir,
    NzbgetRsBuf* out_deleted_old_dest_dir, int* out_has_deleted_old_dest_dir,
    int* out_clear_output_filename);


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

int nzbget_rs_xmlrpc_match_command(
    int user_access, int ua_add, int ua_restricted, const char* method_name);
int nzbget_rs_xmlrpc_is_safe_command(int cmd_id);
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
    int active_downloads, int has_post_info, int post_stage,
    int has_queue_script_job, int queue_script_active,
    int direct_unpack_status, int kind);
NzbgetRsBuf nzbget_rs_xmlrpc_format_postqueue_item_start(
    int is_json, int index, int nzb_id, const char* info_name, int stage, int file_progress);
NzbgetRsBuf nzbget_rs_xmlrpc_format_history_nzb_start(
    int is_json, int index, int id, const char* name, int remaining_file_count,
    int retry_data, int history_time, const char* status);
NzbgetRsBuf nzbget_rs_xmlrpc_format_history_dup_item(
    int is_json, int index, int id, const char* name, int history_time, int64_t size,
    const char* dupe_key, int dupe_score, int dupe_mode, int dup_status);
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

NzbgetRsBuf nzbget_rs_nserv_format_usage(const char* base_name);

void nzbget_rs_binrpc_init_message_base(
    uint8_t* out, int request_type, int struct_size,
    const char* username, const char* password);
void nzbget_rs_binrpc_decode_log_response_entries(
    const uint8_t* data, size_t len, uint32_t nr_entries, void* ctx,
    void (*emit_cb)(void* ctx, uint32_t id, uint32_t kind, uint32_t time, const char* text));
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
NzbgetRsBuf nzbget_rs_tpr_article_writer_format_dest_file_path(
    const char* dest_dir, char sep, const char* filename);
NzbgetRsBuf nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
    int32_t first_idx, const int64_t* values, size_t count);
NzbgetRsBuf nzbget_rs_dqh_fs_extract_parent_dir_path(const char* filename, char sep);
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
int nzbget_rs_dqh_parse_c_atoi(const char* text);
int nzbget_rs_dqh_diskstate_parse_queue_file_id_and_suffix(
    const char* filename, int* out_id, char* out_suffix);
int nzbget_rs_dupe_util_write_atomic(
    const char* path, size_t path_len, const char* data, size_t data_len);
NzbgetRsBuf nzbget_rs_dupe_util_read_all(const char* path, size_t path_len);
int nzbget_rs_dqh_diskstate_next_comma_token(
    const char* s, size_t cursor, size_t* out_tok_start,
    size_t* out_tok_len, size_t* out_next_cursor);
char* nzbget_rs_dqh_tokenizer_next(
    char* data_string, const char* separators, int working, char** save_ptr);
uint32_t nzbget_rs_dqh_article_writer_step_file_crc(
    int first_article, uint32_t current_crc, uint32_t article_crc,
    int segment_size, int* out_first_article);
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
int nzbget_rs_feed_parse_season_episode(
    const char* title, NzbgetRsBuf* out_season, NzbgetRsBuf* out_episode);

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
NzbgetRsBuf nzbget_rs_dupecoord_no_backup_reason(
    int fails_over_flag, int dupe_mode, const NzbgetRsDupeHistoryCandidate* history,
    size_t history_len, void* cb_ctx, int (*file_exists_cb)(void* ctx, size_t history_index));
int nzbget_rs_dupecoord_is_downloaded_on_disk_candidate(
    int health_check, const char* target_dupe_key, int is_self, int delete_status,
    int is_dupe_success, int mark_status, const char* item_dupe_key,
    const char* item_final_dir, const char* item_dest_dir, const char* ext_cleanup_disk);
int nzbget_rs_dupecoord_accumulate_queue_dupe_status(
    int current_statuses, const char* name, const char* dupe_key,
    const char* item_name, const char* item_dupe_key,
    int success_articles, int failed_articles);
int nzbget_rs_dupecoord_accumulate_history_dupe_status(
    int current_statuses, int kind, const char* name, const char* dupe_key,
    const char* item_name, const char* item_dupe_key,
    const char* nzb_text_status, int dup_status);

int nzbget_rs_unpack_has_completed_archive_ext(const char* filename);

void nzbget_rs_queue_editor_visit_category_ext_diff(
    const char* old_ext, const char* new_ext, void* ctx,
    void (*cb)(void* ctx, const char* param_name, const char* param_val));

int nzbget_rs_history_coord_should_recheck_failed_articles(
    int daf_none, int hc_dupe, int kind_nzb, int failed_articles,
    const char* text_status, int has_dupe_recheck_param);
void nzbget_rs_history_coord_prepare_requeued_loaded_file(
    const char* dest_dir, char path_sep, const char* recorded_name,
    const char* current_file_filename, int success_articles, int current_partial_state,
    int ps_none, int ps_completed, int reprocess, int reset_failed,
    const int* article_statuses, size_t article_statuses_len,
    int ai_undefined, int ai_failed, void* ctx,
    int64_t (*file_size_cb)(void* ctx, const char* path),
    void (*delete_file_cb)(void* ctx, const char* path),
    int (*file_exists_cb)(void* ctx, const char* path),
    int* out_set_recorded_filename, int* out_new_partial_state,
    int* out_warn_missing_on_disk, int* out_no_retryable_articles);
void nzbget_rs_history_coord_reset_articles_batch(
    int all_failed, int reset_failed, int is_par_file, int partial_state,
    int64_t file_size, int64_t file_missed_size, int file_total_articles, int file_missed_articles,
    int64_t* inout_file_failed_size, int* inout_file_failed_articles,
    int64_t* inout_file_remaining_size, int* inout_file_completed_articles,
    int64_t* inout_file_success_size, int* inout_file_success_articles,
    int64_t* inout_nzb_failed_size, int64_t* inout_nzb_success_size,
    int* inout_nzb_failed_articles, int* inout_nzb_success_articles,
    int64_t* inout_nzb_par_failed_size, int64_t* inout_nzb_par_success_size,
    int* inout_article_statuses, const int* article_sizes, uint8_t* out_reset_flags,
    size_t articles_len);
int nzbget_rs_diskstate_save_id_floor(
    const char* queue_dir, char sep, int nzb_id, int file_id);

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

int nzbget_rs_stream_eval_build_repair_job(
    int fallback_mode, int daf_stream, int raw_article, int is_par_file,
    const char* disk_basename, int has_nzb, int delete_status, int ds_none,
    int ds_health, int deleting, int parking, int has_post_info,
    int articles_empty, int success_articles, int64_t encoded_size,
    int64_t decoded_file_size);
int nzbget_rs_stream_watchdog_eval_tick(
    int64_t progress_bytes, int64_t prev_progress, int waiting, int materializing,
    int64_t since_last_progress_sec, int64_t since_begun_sec, int64_t since_start_sec,
    double elapsed_sec, int timeout_sec, int max_run_factor, int max_conn_wait_sec,
    int slow_repair_factor, int64_t download_rate, int64_t start_progress,
    int64_t missing_bytes, int64_t* out_new_progress, int* out_reset_last_progress,
    NzbgetRsBuf* out_reason);
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

size_t nzbget_rs_dupematcher_filter_dupe_sources(
    const char* nzb_name, const char* dest_dir, int64_t expected_size, const char* unrar_cmd,
    const char* const* dupe_dirs, size_t dupe_count, size_t* out_indices,
    void (*on_message)(void* ctx, int kind, const char* msg), void* ctx);

int nzbget_rs_archive_processor_process(
    const char* archive_file, const char* unpack_dir, const char* dest_dir,
    const char* processed_dir, const char* broken_dir, int config_action, void* ctx,
    void (*log_cb)(void* ctx, int kind, const char* msg),
    void (*moved_cb)(void* ctx, const char* moved_path));

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

int nzbget_rs_benchmark_run(
    const char* dir_ptr, size_t dir_len, size_t block_size_bytes,
    uint64_t max_file_size_bytes, int64_t timeout_secs,
    int space_ok, uint64_t available_bytes,
    const char* space_err_ptr, size_t space_err_len,
    uint64_t* out_write_bytes, double* out_write_ms,
    uint64_t* out_read_bytes, double* out_read_ms,
    NzbgetRsBuf* out_err);

int64_t nzbget_rs_util_safe_int_cast_i64_to_i64(int64_t num, int64_t min_to, int64_t max_to);
int64_t nzbget_rs_util_safe_int_cast_u64_to_i64(uint64_t num, uint64_t max_to);
uint64_t nzbget_rs_util_safe_int_cast_i64_to_u64(int64_t num, uint64_t max_to);
uint64_t nzbget_rs_util_safe_int_cast_u64_to_u64(uint64_t num, uint64_t max_to);

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

typedef struct NzbgetRsTransientRetries NzbgetRsTransientRetries;
NzbgetRsTransientRetries* nzbget_rs_transient_retries_new(int attempts, int interval_sec);
void nzbget_rs_transient_retries_free(NzbgetRsTransientRetries* ptr);
int nzbget_rs_transient_retries_register(NzbgetRsTransientRetries* ptr, uintptr_t server_key, int64_t now);
size_t nzbget_rs_transient_retries_waiting_servers(const NzbgetRsTransientRetries* ptr, int64_t now, uintptr_t* out_keys, size_t max_keys);
int nzbget_rs_transient_retries_is_empty(const NzbgetRsTransientRetries* ptr);
int nzbget_rs_transient_retries_is_exhausted(const NzbgetRsTransientRetries* ptr);
int nzbget_rs_transient_retries_remained_retries(const NzbgetRsTransientRetries* ptr, uintptr_t server_key);
void nzbget_rs_transient_retries_set_remained_retries(NzbgetRsTransientRetries* ptr, uintptr_t server_key, int remained);

int64_t nzbget_rs_fs_file_size(const char* filename);
int nzbget_rs_fs_delete_file(const char* filename);
int nzbget_rs_fs_create_directory_exclusive(const char* dir_filename);
int nzbget_rs_fs_remove_directory(const char* dir_filename);
int nzbget_rs_fs_open_cloexec_fd(const char* filename, int mode, int om_read, int om_read_write, int om_write);
int nzbget_rs_fs_flush_file_buffers(int file_descriptor, NzbgetRsBuf* out_errmsg);
int nzbget_rs_fs_flush_dir_buffers(const char* filename, char path_sep, NzbgetRsBuf* out_errmsg);
int nzbget_rs_fs_allocate_file(const char* filename, int64_t size, NzbgetRsBuf* out_errmsg);
int nzbget_rs_fs_move_file(const char* src_filename, const char* dst_filename);
int nzbget_rs_fs_move_file_no_overwrite(const char* src_filename, const char* dst_filename);
int nzbget_rs_fs_get_disk_state(
    const char* path,
    int64_t* out_available,
    int64_t* out_total);

void nzbget_rs_bstring_set(char* buf, int size, const char* str_ptr, int len);
void nzbget_rs_bstring_append(char* buf, int size, const char* str_ptr, int len);
int nzbget_rs_bstring_format(
    char* buf, int size, void* ctx, int (*fmt_cb)(void*, char*, size_t));
int nzbget_rs_cstring_eq(const char* a, const char* b);
void nzbget_rs_cstring_clear(char** m_data);
void nzbget_rs_cstring_bind(char** m_data, char* str_ptr);
void nzbget_rs_cstring_set(char** m_data, const char* str_ptr, int len);
int nzbget_rs_cstring_format(
    char** m_data, int new_len_no_nul, void* ctx, int (*fmt_cb)(void*, char*, size_t));
void nzbget_rs_cstring_reserve(char** m_data, int capacity);
void nzbget_rs_string_builder_clear(char** m_data, int* m_length, int* m_capacity);
void nzbget_rs_string_builder_reserve(char** m_data, int* m_capacity, int capacity, int exact);
void nzbget_rs_string_builder_append(
    char** m_data, int* m_length, int* m_capacity, const char* str_ptr, int len);
void nzbget_rs_char_buffer_reserve(char** m_data, int* m_size, int size);
void nzbget_rs_char_buffer_clear(char** m_data, int* m_size);

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

NzbgetRsBuf nzbget_rs_xmlrpc_normalize_url(const char* url);
NzbgetRsBuf nzbget_rs_xmlrpc_execute_save_config(
    char** request, int is_get, int is_json,
    const char* config_filename, int* out_fault);
NzbgetRsBuf nzbget_rs_xmlrpc_execute_load_config(
    int is_json, int user_access, int ua_restricted,
    const char* config_filename, int* out_fault);
NzbgetRsBuf nzbget_rs_xmlrpc_execute_tls_cipher(
    int operation, int user_access, int ua_control,
    int protocol, int rp_json_rpc, int http_method, int hm_post,
    int is_json, const char* request, NzbgetRsJsonLimits limits,
    void* ctx, NzbgetRsBuf (*backend_cb)(void* ctx, int op_kind, const char* job_id),
    int* out_fault);
int nzbget_rs_xmlrpc_parse_edit_queue(
    char** request, int is_get, int is_json,
    int ea_file_move_offset, int ea_group_move_offset,
    int* out_action, NzbgetRsBuf* out_args_or_err, int* out_err_code,
    void* id_ctx, void (*add_id_cb)(void* ctx, int id));
int nzbget_rs_xmlrpc_parse_start_script(
    char** request, int is_get, int is_json,
    NzbgetRsBuf* out_script, NzbgetRsBuf* out_command, NzbgetRsBuf* out_context,
    void* entry_ctx, void (*add_entry_cb)(void* ctx, const char* name, const char* val));
int nzbget_rs_xmlrpc_parse_test_disk_speed(
    char** request, int is_get, int is_json,
    NzbgetRsBuf* out_dir_path, size_t* out_buffer_bytes, uint64_t* out_max_file_bytes,
    int* out_timeout_sec, const char** out_err_msg);

int nzbget_rs_xmlrpc_parse_append_fleet(
    char** request, int is_get, int is_json, int max_members,
    void* ctx,
    void (*header_cb)(void* ctx, const char* dupe_key, const char* category, int priority, int timeout_sec),
    void (*member_cb)(void* ctx, const char* name, int is_url, const char* data, size_t data_len),
    NzbgetRsBuf* out_err);

typedef struct NzbgetRsParsedDownload
{
    int v13;
    NzbgetRsBuf filename;
    NzbgetRsBuf category;
    int priority;
    int add_top;
    int is_url;
    NzbgetRsBuf content;
    int add_paused;
    int has_dupe_key;
    NzbgetRsBuf dupe_key;
    int dupe_score;
    int dupe_mode;
    int auto_category;
} NzbgetRsParsedDownload;

int nzbget_rs_xmlrpc_parse_download(
    char** request, int is_get, int is_json,
    int dm_score, int dm_all, int dm_force,
    NzbgetRsParsedDownload* out_dl,
    void* param_ctx, void (*add_param_cb)(void* ctx, const char* name, const char* val),
    const char** out_err_msg);

typedef struct NzbgetRsParsedViewFeed
{
    int id;
    int include_non_matching;
    NzbgetRsBuf name;
    NzbgetRsBuf url;
    NzbgetRsBuf filter;
    int backlog;
    int pause_nzb;
    NzbgetRsBuf category;
    int category_source;
    int priority;
    int interval;
    int has_feed_filter;
    NzbgetRsBuf feed_filter;
    int cache_time_sec;
    NzbgetRsBuf cache_id;
} NzbgetRsParsedViewFeed;

int nzbget_rs_xmlrpc_parse_view_feed(
    char** request, int is_get, int is_json, int preview,
    int cs_nzb_file, int cs_feed_file, int cs_auto,
    NzbgetRsParsedViewFeed* out_vf);

typedef struct NzbgetRsParsedTestServer
{
    NzbgetRsBuf host;
    NzbgetRsBuf username;
    NzbgetRsBuf password;
    NzbgetRsBuf cipher;
    int port;
    int timeout;
    int cert_verif_level;
    int encryption;
} NzbgetRsParsedTestServer;

int nzbget_rs_xmlrpc_parse_test_server(
    const char* request_body, char** request_ptr,
    int is_get, int is_json, int http_method, int hm_post, int cert_verif_count,
    NzbgetRsJsonLimits limits,
    NzbgetRsParsedTestServer* out_ts, const char** out_err_msg);

int nzbget_rs_unpack_can_extract_with_zipfast(const char* archive_path);
int nzbget_rs_unpack_extract_with_zipfast(
    const char* archive_path, const char* output_dir, const char* password,
    int overwrite_mode, int cleanup_disk,
    void (*log_cb)(void* ctx, int kind, const char* msg), void* log_ctx,
    int (*is_stopped_cb)(void* ctx));

void nzbget_rs_fs_fix_exec_permission(const char* filename);

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
uint32_t nzbget_rs_gzip_len(int input_len);
uint32_t nzbget_rs_gzip_compress(
    const uint8_t* in_buf, int in_len, uint8_t* out_buf, int out_len);
typedef struct RsGUnzipStream RsGUnzipStream;
RsGUnzipStream* nzbget_rs_gunzip_new(int buf_size);
void nzbget_rs_gunzip_free(RsGUnzipStream* stream);
void nzbget_rs_gunzip_write(RsGUnzipStream* stream, const uint8_t* in_buf, int in_len);
int nzbget_rs_gunzip_read(RsGUnzipStream* stream, const void** out_ptr, int* out_len);

int nzbget_rs_term_init(void);
void nzbget_rs_term_shutdown(void);
void nzbget_rs_term_get_size(int* rows, int* cols);
void nzbget_rs_term_clear(void);
void nzbget_rs_term_plot_text(
    const char* text, int row, int col, int use_color, int color_pair, int blink);
void nzbget_rs_term_refresh(int row, int col);
int nzbget_rs_term_read_key(void);

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

void nzbget_rs_enable_core_dump(void);
int nzbget_rs_socket_deadline_wait(
    int fd, int read, int write, void* ctx,
    int64_t (*remaining_ms_or_stopped_cb)(void* ctx));
int nzbget_rs_daemonize_setup(
    const char* lock_file, const char* daemon_username,
    int* out_lock_owned, int* out_warn_truncate);
void nzbget_rs_disk_service_check_disk_space(
    const char* dest_dir, const char* inter_dir, int disk_space_mb,
    int* out_dest_low, int* out_inter_low);
int nzbget_rs_disk_service_check_required_dirs(
    const char* required_dir, int waiting_reported, int* out_next_waiting_reported,
    void* ctx, void (*on_missing_cb)(void* ctx, const char* dir));
NzbgetRsBuf nzbget_rs_http_get_read_and_delete(
    const char* path, size_t path_len, size_t max_bytes, int read_body, int* out_ok);
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
int32_t nzbget_rs_scheduler_calc_next_work_interval(int64_t cur_time);
int nzbget_rs_fs_save_buffer_into_file(
    const char* filename, const char* buffer, size_t buf_len);
NzbgetRsBuf nzbget_rs_fs_expand_file_name(const char* filename);
NzbgetRsBuf nzbget_rs_fs_get_exe_file_name(const char* argv0);
int nzbget_rs_util_find_executor_program(
    const char* filename, size_t filename_len,
    const char* custom_path, size_t custom_path_len,
    NzbgetRsBuf* out_buf);
void nzbget_rs_util_format_time(int64_t time_sec, char* buffer, int bufsize);
NzbgetRsBuf nzbget_rs_fs_make_unique_path(const char* target_path);
int nzbget_rs_fs_create_hard_link(
    const char* from, const char* to, NzbgetRsBuf* errmsg);
int nzbget_rs_fs_load_file_into_buffer(
    const char* filename, int add_trailing_null,
    void* ctx, uint8_t* (*reserve_cb)(void* ctx, int buf_size));
NzbgetRsBuf nzbget_rs_fs_expand_home_path(const char* filename);
int nzbget_rs_fs_get_real_path(
    const char* path, size_t path_len, NzbgetRsBuf* out_buf);
NzbgetRsBuf nzbget_rs_fs_get_current_directory(void);
int nzbget_rs_fs_read_first_8_bytes(const char* filename, uint8_t* out_head);
int nzbget_rs_dupecoord_eval_history_dupe_match(
    int32_t kind, uint32_t self_full_hash, uint32_t self_filtered_hash,
    int32_t self_dupe_mode, int32_t self_dupe_score,
    const char* self_name, const char* self_dupe_key,
    uint32_t item_full_hash, uint32_t item_filtered_hash,
    int32_t item_dupe_mode, int32_t item_mark_status, int32_t item_dup_status, int32_t item_dupe_score,
    const char* item_name, const char* item_dupe_key,
    int32_t* out_same_content, int32_t* out_good);
void nzbget_rs_dupecoord_accum_hc_dupe_same_content_outcome(
    int32_t kind, uint32_t self_full_hash, uint32_t self_filtered_hash,
    uint32_t item_full_hash, uint32_t item_filtered_hash,
    int32_t item_delete_status, int32_t item_is_dupe_success, int32_t item_dup_status,
    int32_t* inout_succeeded, int32_t* inout_failed);
int nzbget_rs_dupecoord_eval_queue_score_dupe(
    int32_t is_self, int32_t queued_delete_status, int32_t queued_is_nzb, int32_t both_are_url,
    int32_t queued_dupe_mode, const char* queued_name, const char* queued_dupe_key,
    const char* self_name, const char* self_dupe_key,
    int32_t self_dupe_score, int32_t queued_dupe_score,
    int32_t health_check, int32_t queued_is_redownload_auto,
    int32_t queued_cur_success_articles, int32_t queued_health, int32_t queued_crit_health,
    int32_t queued_has_post_info);

int nzbget_rs_statmeter_calc_month_slots(int64_t loc_cur_time, int quota_start_day);
int nzbget_rs_unpack_controller_try_password_file(
    const char* path, void* ctx,
    int (*should_continue_cb)(void*),
    void (*try_password_cb)(void*, const char*));

NzbgetRsBuf nzbget_rs_log_rotate_log_file(
    const char* log_file, char sep, int64_t cur_time, int rotate_log,
    void* ctx, void (*on_info_cb)(void*, const char*));
NzbgetRsBuf nzbget_rs_options_find_default_config_file(
    const char* app_dir, char sep, void* ctx,
    int (*file_exists_cb)(void*, const char*), int* out_found);
int nzbget_rs_options_load_config_file(
    const char* path, void* ctx,
    void (*on_line_cb)(void*, int, const char*));

#include "nzbget_tls_cipher.h"

#ifdef __cplusplus
}
#endif

#endif
