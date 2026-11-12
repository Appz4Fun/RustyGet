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

// Release each result exactly once, with all fields unchanged. A zero buffer
// is also accepted. Copy the bytes before freeing if they must outlive the result.
void nzbget_rs_free(NzbgetRsBuf buf);

#include "nzbget_tls_cipher.h"

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

// FileSystem's file operations (rust/src/fsops.rs, POSIX). Buffers returned by
// these functions are Rust-owned and must be freed with nzbget_rs_free, never
// free/delete. Panics abort; they do not unwind across the ABI.
// NULL paths return 0.
int nzbget_rs_file_exists(const char* path);
int nzbget_rs_directory_exists(const char* path);

// ParParser's name checks (rust/src/parparser.rs).
// Names are borrowed and read only through the first NUL or 1023 bytes.
// fold takes a byte (0..255), applies the caller's tolower(char), and must not
// unwind. NULL names/fold return 0. Outputs are optional, writable C ints;
// they may alias each other (blocks is written last), and change only on success.
// No ownership transfer or allocation crosses the ABI; panics abort.
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

// ---- dupesearch (daemon/dupesearch/) ----
// DonorScore (rust/src/donorscore.rs): Base, Target (twin 0/1) and Rerank;

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

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// QueueScript, PostScript, ScanScript, ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/extension.rs`)
// ============================================================================



typedef intptr_t (*NzbgetRsConnHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1,
    const char* s1, const char* s2, const char* s3, char* buf, int* out_int);

typedef struct NzbgetRsNservEnv {
    void* ctx;
    const char* version;
    void (*write_stdout_cb)(const char* data, size_t len);
    void (*log_cb)(void* ctx, int level, const char* msg);
    void (*runtime_cb)(void* ctx, int op, int arg, const char* const* opts, size_t opt_len);
    uintptr_t (*conn_cb)(uintptr_t conn, int op, int a1, int a2, const char* s1, const char* s2, char* out_buf);
    NzbgetRsConnHostFn tls_host_cb;
    int debug_on;
} NzbgetRsNservEnv;
int nzbget_rs_nserv_run(int argc, const char* const* argv, const NzbgetRsNservEnv* env);

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

NzbgetRsBuf nzbget_rs_cmdline_format_usage(
    const char* base_name, char path_sep, int include_daemon);

typedef struct NzbgetRsOptions NzbgetRsOptions;
typedef struct NzbgetRsFeedCoord NzbgetRsFeedCoord;
typedef struct NzbgetRsScheduler NzbgetRsScheduler;
typedef struct NzbgetRsLog NzbgetRsLog;
typedef struct NzbgetRsCmdScriptLog NzbgetRsCmdScriptLog;
typedef struct NzbgetRsStatMeter NzbgetRsStatMeter;
typedef struct NzbgetRsWorkState NzbgetRsWorkState;
typedef struct NzbgetRsScriptConfig NzbgetRsScriptConfig;
typedef struct NzbgetRsServerPool NzbgetRsServerPool;
typedef struct NzbgetRsServiceCoord NzbgetRsServiceCoord;
typedef int (*NzbgetRsServiceCoordFn)(void* ctx, void* svc, int op);

NzbgetRsWorkState* nzbget_rs_workstate_new(uintptr_t notify_ctx, void (*notify_cb)(uintptr_t));
void nzbget_rs_workstate_free(NzbgetRsWorkState* ws);
int64_t nzbget_rs_workstate_ctl(const NzbgetRsWorkState* ws, int id, int write, int64_t val);

NzbgetRsServiceCoord* nzbget_rs_service_coord_new(void);
void nzbget_rs_service_coord_free(NzbgetRsServiceCoord* h);
void nzbget_rs_service_coord_ctl(NzbgetRsServiceCoord* h, int op, void* ctx, NzbgetRsServiceCoordFn cb);

NzbgetRsServerPool* nzbget_rs_serverpool_new(NzbgetRsConnHostFn conn_host_cb);
void nzbget_rs_serverpool_free(NzbgetRsServerPool* pool);
void nzbget_rs_serverpool_add(
    const NzbgetRsServerPool* pool, int id, int active, const char* name, const char* host,
    int port, int ip_version, const char* user, const char* pass, int join_group, int tls,
    const char* cipher, int max_connections, int retention, int level, int group,
    int optional, uint32_t cert_verif_level);
intptr_t nzbget_rs_serverpool_ctl(
    const NzbgetRsServerPool* pool, int op, int a1, int a2, uintptr_t p1, uintptr_t p2);
int nzbget_rs_serverpool_test(
    const char* host, int port, const char* user, const char* pass, int tls,
    const char* cipher, int timeout, uint32_t cert_verif_level,
    NzbgetRsConnHostFn conn_host_cb, NzbgetRsBuf* out_err);
int nzbget_rs_serverpool_probe_tls(
    const NzbgetTlsServer* server, const char* cipher, int expected_ver,
    const char* numeric, uintptr_t deadline_ptr, NzbgetRsConnHostFn conn_host_cb);

NzbgetRsBuf nzbget_rs_system_health_diagnose(
    const NzbgetRsOptions* opts_ptr,
    const NzbgetRsServerPool* pool_ptr,
    const NzbgetRsFeedCoord* feed_coord_ptr,
    const NzbgetRsScheduler* sched_ptr,
    const NzbgetRsLog* log_ptr,
    int daemon_mode, int format_mode);

/* Extension & Script Subsystem End-to-End C ABI (`rust/src/extension.rs`, `rust/src/script.rs`, `rust/src/options.rs`) */

typedef struct NzbgetRsExtManager NzbgetRsExtManager;
typedef struct NzbgetRsScriptRunner NzbgetRsScriptRunner;
typedef uintptr_t (*NzbgetRsScriptHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, const char** strs);
NzbgetRsExtManager* nzbget_rs_ext_mgr_new(void);
void nzbget_rs_ext_mgr_free(NzbgetRsExtManager* mgr);
int nzbget_rs_ext_mgr_op(
    NzbgetRsExtManager* mgr, int op, int a1, int a2, uintptr_t p1,
    const char* s1, const char* s2, const char* s3, const char* const* strs,
    void* ctx, NzbgetRsScriptHostFn host_cb, NzbgetRsBuf* out_buf);
NzbgetRsScriptRunner* nzbget_rs_script_runner_new(void);
void nzbget_rs_script_runner_free(NzbgetRsScriptRunner* r);
void nzbget_rs_script_runner_terminate(NzbgetRsScriptRunner* r, NzbgetRsScriptHostFn host_cb);
void nzbget_rs_script_terminate_all(NzbgetRsScriptHostFn host_cb);
void nzbget_rs_script_run_job(uintptr_t job_ptr, NzbgetRsScriptHostFn host_cb);
int nzbget_rs_command_script_start(
    const NzbgetRsExtManager* ext_mgr, const char* script_name, const char* command,
    int log_id, const NzbgetRsRpcParamItem* params, size_t params_len,
    NzbgetRsScriptHostFn host_cb);
int nzbget_rs_feed_scripts_exec(
    const NzbgetRsExtManager* ext_mgr, const char* feed_script, const char* feed_file,
    int feed_id, NzbgetRsScriptHostFn host_cb);
void nzbget_rs_scheduler_script_start(
    const NzbgetRsExtManager* ext_mgr, const char* param, int external_process,
    int task_id, const char* const* words, size_t word_count, NzbgetRsScriptHostFn host_cb);
void nzbget_rs_scan_scripts_exec(
    const NzbgetRsExtManager* ext_mgr, const char* extensions, const char* nzb_filename,
    const char* url, const char* directory, void* scan_ctx,
    void (*scan_cb)(void*, int, int, const char*, const char*, int*, const char**),
    NzbgetRsScriptHostFn host_cb);

typedef struct NzbgetRsQueueScriptCoord NzbgetRsQueueScriptCoord;
typedef struct NzbgetRsQueueScriptCoordEnv {
    void* ctx; const NzbgetRsExtManager* ext_mgr; uintptr_t p1;
    int a1; int a2; int a3; int a4; int64_t t1; int64_t t2;
    const char* s1; int* out_i32; NzbgetRsScriptHostFn host_cb;
    uintptr_t (*action_cb)(void*, int, int, uintptr_t, const char*, int*, const char**);
} NzbgetRsQueueScriptCoordEnv;
NzbgetRsQueueScriptCoord* nzbget_rs_queue_script_new(void);
void nzbget_rs_queue_script_free(NzbgetRsQueueScriptCoord* h);
int nzbget_rs_queue_script_exec(NzbgetRsQueueScriptCoord* h, int op, const NzbgetRsQueueScriptCoordEnv* env);

/* ============================================================================
 * Feed Subsystem (`rust/src/feed.rs`)
 * ============================================================================ */

typedef struct NzbgetRsFeedCoordEnv {
    void* ctx; uintptr_t p1; int a1; int a2; int a3; int a4; int a5; int a6; uint32_t u1;
    const char* s1; const char* s2; const char* s3; const char* s4; const char* s5; const char* s6;
    void (*log_cb)(void* ctx, int level, const char* msg);
    uintptr_t (*query_cb)(void* ctx, int op, uintptr_t ptr, int* out_ints, int64_t* out_i64s, const char** out_strs);
    uintptr_t (*action_cb)(void* ctx, int op, uintptr_t p1, int a1, int a2, int a3, int64_t t1, int64_t t2, const char* s1, const char* s2, const char* s3, const char* s4);
    void (*rpc_emit_cb)(void* ctx, const NzbgetRsRpcFeedItem* item);
} NzbgetRsFeedCoordEnv;
NzbgetRsFeedCoord* nzbget_rs_feed_coord_new(void);
void nzbget_rs_feed_coord_free(NzbgetRsFeedCoord* coord);
int nzbget_rs_feed_coord_exec(const NzbgetRsFeedCoord* coord, int op, const NzbgetRsFeedCoordEnv* env);

typedef struct NzbgetRsUrlCoord NzbgetRsUrlCoord;
typedef struct NzbgetRsUrlCoordEnv {
    void* ctx; uintptr_t p1; uintptr_t p2; int a1; int a2; const char* s1;
    void (*with_queue_guard_cb)(void* ctx, void* inner_ctx, void (*fn_cb)(void* inner_ctx, uintptr_t dq_ptr));
    void (*log_cb)(void* ctx, int level, const char* msg);
    uintptr_t (*query_cb)(void* ctx, int op, uintptr_t ptr, int* out_ints, int64_t* out_i64s, const char** out_strs);
    uintptr_t (*action_cb)(void* ctx, int op, uintptr_t p1, uintptr_t p2, int a1, int a2, const char* s1, const char* s2, const char* s3);
} NzbgetRsUrlCoordEnv;
NzbgetRsUrlCoord* nzbget_rs_url_coord_new(void);
void nzbget_rs_url_coord_free(NzbgetRsUrlCoord* coord);
int nzbget_rs_url_coord_exec(const NzbgetRsUrlCoord* coord, int op, const NzbgetRsUrlCoordEnv* env);

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

NzbgetRsBuf nzbget_rs_alloc_buf(const char* data, size_t len);
NzbgetRsBuf nzbget_rs_system_info_format(
    int mode, int with_network, int use_tls, int include_gzip,
    const char* version_rev, const char* ssl_lib_name,
    const char* ssl_raw_version, int ssl_extract_after_space,
    NzbgetRsConnHostFn conn_host_cb,
    const NzbgetRsLog* log, const NzbgetRsOptions* opts);

NzbgetRsScriptConfig* nzbget_rs_script_config_new(void);
void nzbget_rs_script_config_free(NzbgetRsScriptConfig* sc);

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

typedef void (*NzbgetRsOptionsHostFn)(
    void* ctx, int op, const int* ints, const char* const* strs, size_t count);
NzbgetRsOptions* nzbget_rs_options_new(
    const char* exe_name, const char* config_filename, int no_config, int no_disk_access,
    const char* const* cmd_opts, size_t cmd_opts_len, int flags, const char* version,
    void* cb_ctx, NzbgetRsOptionsHostFn cb_fn,
    NzbgetRsParsedOptions* out_scalars, int* out_state_flags);
void nzbget_rs_options_free(NzbgetRsOptions* handle);
const char* nzbget_rs_options_str(const NzbgetRsOptions* handle, int id);
int nzbget_rs_options_task(
    const NzbgetRsOptions* handle, int id, const char* time, const char* week_days,
    int command, const char* param, void* cb_ctx, NzbgetRsOptionsHostFn cb_fn);
int nzbget_rs_options_query(
    const NzbgetRsOptions* handle, int op, const char* s1, int a1,
    int* out_int, const char** out_strs, uintptr_t ctx,
    void (*cb)(uintptr_t, const char*, const char*, int));

NzbgetRsLog* nzbget_rs_log_new(int debug_build);
void nzbget_rs_log_free(NzbgetRsLog* log);
void nzbget_rs_log_emit(
    const NzbgetRsLog* log, int kind, const NzbgetRsOptions* options, const char* text);
void nzbget_rs_log_ctl(
    const NzbgetRsLog* log, int op, int a1, uintptr_t p1,
    const NzbgetRsOptions* options, const char* s1, void (*debug_cb)(uintptr_t));
void nzbget_rs_log_collect(
    int mode, const NzbgetRsLog* log, const NzbgetRsCmdScriptLog* cmd_log,
    int a1, int a2, uint32_t u1, int* out_int, void* ctx, uintptr_t fn1, uintptr_t fn2);
uintptr_t nzbget_rs_cmd_script_log_op(
    int op, NzbgetRsCmdScriptLog* handle, int script_id, int kind, const char* text);

uint32_t nzbget_rs_crc32_append(const uint8_t* block, uint32_t len, uint32_t init);

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

int nzbget_rs_fs_load_file_into_buffer(
    const char* filename, int add_trailing_null, void* ctx,
    uint8_t* (*reserve_cb)(void*, int));
int nzbget_rs_fs_get_disk_state(
    const char* path, int64_t* out_avail, int64_t* out_total);
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
    void (*aux_cb)(void* ctx, int op, int a1, int a2, uint32_t u1, const char* s1, const char* s2, const void* vol_item, void* sink_ctx, void (*sink_fn)(void*, int, int, int, uint32_t, const char*, const void*, size_t, const void*, size_t));
} NzbgetRsDiskStateEnv;

int nzbget_rs_diskstate_exec(int op, const NzbgetRsDiskStateEnv* env);
NzbgetRsStatMeter* nzbget_rs_statmeter_new(const NzbgetRsLog* log, const NzbgetRsOptions* opts);
void nzbget_rs_statmeter_free(NzbgetRsStatMeter* sm);
int64_t nzbget_rs_statmeter_speed(const NzbgetRsStatMeter* sm, int op, int bytes);
int nzbget_rs_statmeter_ctl(
    const NzbgetRsStatMeter* sm, int op, int a1, int a2,
    uint32_t u1, uint32_t u2, uint32_t u3,
    const NzbgetRsOptions* opts, const NzbgetRsLog* log,
    int* out_ints, int64_t* out_i64s);
int nzbget_rs_statmeter_disk(
    const NzbgetRsStatMeter* sm, int op, int server_mode, int local_time_offset,
    const NzbgetRsOptions* opts, const NzbgetRsLog* log,
    const NzbgetRsServerPool* pool, int* inout_perfect);
typedef struct NzbgetRsDupeProbe NzbgetRsDupeProbe;
typedef uintptr_t (*NzbgetRsDupeProbeHostFn)(void*, int, int, int, int, uintptr_t, uintptr_t, const char*, int*, const char**);
NzbgetRsDupeProbe* nzbget_rs_dupe_probe_new(int nzb_id, int count_all, int recovered_at_start);
void nzbget_rs_dupe_probe_free(NzbgetRsDupeProbe* p);
int nzbget_rs_dupe_probe_exec(NzbgetRsDupeProbe* p, int op, size_t n, int a1, int a2, uintptr_t p1, const char* s1, const char* const* groups, size_t* out_idx, void* ctx, NzbgetRsDupeProbeHostFn host_cb);
typedef struct NzbgetRsHistoryCoordEnv {
    void* ctx; uintptr_t p1; uintptr_t p2;
    int a1; int a2; int a3; int a4; int a5; int a6; int64_t t1;
    const char* s1; const int* int_arr; size_t int_len;
    void (*log_cb)(void*, int, const char*);
    uintptr_t (*action_cb)(void*, int, int, int, uintptr_t, uintptr_t, const char*, const char*, int*, int64_t*, const char**, uint8_t*, size_t);
} NzbgetRsHistoryCoordEnv;
int nzbget_rs_history_coord_exec(int op, const NzbgetRsHistoryCoordEnv* env);
typedef uintptr_t (*NzbgetRsTwinHostFn)(
    int op, int a1, int a2, uintptr_t p1,
    const char* s1, const char* s2, const char* s3, const char* s4,
    int* ints, int64_t* i64s, const char** strs);
int nzbget_rs_twin_exec(
    int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, const char* s3, const char* s4,
    NzbgetRsTwinHostFn host_cb, NzbgetRsBuf* out_buf);
int nzbget_rs_socket_deadline_wait(
    int fd, int read, int write, void* ctx, int64_t (*rem_cb)(void*));
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
typedef struct NzbgetRsSchedulerEnv {
    void* ctx; int a1; int a2; int a3; int a4; int a5; const char* s1;
    void (*log_cb)(void*, int, const char*);
    int (*action_cb)(void*, int, int, int, const char* const*, size_t, int*, int64_t*, const char**);
} NzbgetRsSchedulerEnv;
NzbgetRsScheduler* nzbget_rs_scheduler_new(void);
void nzbget_rs_scheduler_free(NzbgetRsScheduler* h);
int nzbget_rs_scheduler_exec(NzbgetRsScheduler* h, int op, const NzbgetRsSchedulerEnv* env);

typedef struct NzbgetRsDaemonEnv {
    void* ctx; int a1; int a2; int a3; int a4; int a5; int a6;
    const char* s1; const char* s2; const char* s3; const char* s4; const char* s5;
    void (*segv_cb)(void*, const char*); void (*global_log_cb)(int, const char*); int (*reload_cb)(int);
    void (*log_cb)(void*, int, const char*); int (*action_cb)(void*, int, int, const char*, const char*);
} NzbgetRsDaemonEnv;
int nzbget_rs_daemon_exec(int op, const NzbgetRsDaemonEnv* env);
typedef struct NzbgetRsRepairController NzbgetRsRepairController;
typedef uintptr_t (*NzbgetRsRepairCtrlHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);
NzbgetRsRepairController* nzbget_rs_repair_ctrl_new(void);
void nzbget_rs_repair_ctrl_free(NzbgetRsRepairController* ctrl);
int nzbget_rs_repair_ctrl_exec(
    const NzbgetRsRepairController* ctrl, int op, int a1, uintptr_t p1,
    NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
typedef struct NzbgetRsParChecker NzbgetRsParChecker;
NzbgetRsParChecker* nzbget_rs_par_checker_new(void);
void nzbget_rs_par_checker_free(NzbgetRsParChecker* ctrl);
int nzbget_rs_par_checker_exec(
    const NzbgetRsParChecker* ctrl, int op, int a1, const char* s1,
    NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
typedef struct NzbgetRsUnpackController NzbgetRsUnpackController;
NzbgetRsUnpackController* nzbget_rs_unpack_ctrl_new(void);
void nzbget_rs_unpack_ctrl_free(NzbgetRsUnpackController* ctrl);
int nzbget_rs_unpack_ctrl_exec(
    const NzbgetRsUnpackController* ctrl, int op,
    NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
typedef struct NzbgetRsDirectUnpack NzbgetRsDirectUnpack;
NzbgetRsDirectUnpack* nzbget_rs_direct_unpack_new(int nzb_id);
void nzbget_rs_direct_unpack_free(NzbgetRsDirectUnpack* ctrl);
int nzbget_rs_direct_unpack_exec(
    const NzbgetRsDirectUnpack* ctrl, int op, uintptr_t p1, uintptr_t p2,
    NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
int nzbget_rs_post_job_exec(
    int job, int a1, int a2, NzbgetRsRepairCtrlHostFn host_cb, void* ctx);
void nzbget_rs_post_script_run(
    NzbgetRsScriptRunner* runner, const NzbgetRsExtManager* ext_mgr, int parcheck_enabled,
    void* ctx, NzbgetRsRepairCtrlHostFn host_cb, NzbgetRsScriptHostFn script_host_cb);
typedef struct NzbgetRsStreamRepairCtrl NzbgetRsStreamRepairCtrl;
typedef uintptr_t (*NzbgetRsStreamRepairHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, int* ints, int64_t* i64s, const char** strs);
NzbgetRsStreamRepairCtrl* nzbget_rs_stream_repair_ctrl_new(
    int live_mode, int nzb_id,
    NzbgetRsDupeProbeHostFn probe_host, int tls_enabled, int debug_enabled);
void nzbget_rs_stream_repair_ctrl_free(NzbgetRsStreamRepairCtrl* ctrl);
int nzbget_rs_stream_repair_ctrl_exec(
    const NzbgetRsStreamRepairCtrl* ctrl, int op,
    NzbgetRsStreamRepairHostFn host_cb, void* ctx);

typedef struct NzbgetRsCancelReg NzbgetRsCancelReg;
NzbgetRsCancelReg* nzbget_rs_cancel_reg_new(void);
void nzbget_rs_cancel_reg_stop(const NzbgetRsCancelReg* reg);
void nzbget_rs_cancel_reg_free(NzbgetRsCancelReg* reg);
typedef struct NzbgetRsConn NzbgetRsConn;
void nzbget_rs_conn_free(NzbgetRsConn* conn);
intptr_t nzbget_rs_conn_ctl(
    NzbgetRsConn* conn, int op, int a1, uintptr_t p1,
    const char* s1, const char* s2, const char* const* addrs, size_t addrs_len);
int nzbget_rs_conn_io(
    NzbgetRsConn* conn, int op, char* buf, int size, int* out_int);
typedef struct NzbgetRsWebDl NzbgetRsWebDl;
typedef int (*NzbgetRsWebDlHostFn)(
    void* ctx, int op, int a1, const char* s1, int* out_ints, const char** out_str);
NzbgetRsWebDl* nzbget_rs_webdl_new(
    int debug_on, NzbgetRsConnHostFn conn_host_cb, void* webdl_ctx, NzbgetRsWebDlHostFn webdl_cb);
void nzbget_rs_webdl_free(NzbgetRsWebDl* dl);
intptr_t nzbget_rs_webdl_ctl(
    NzbgetRsWebDl* dl, int op, int a1, const char* s1, int64_t* out_i64, NzbgetRsBuf* out_buf);
int nzbget_rs_webdl_read_temp_url(
    const char* url, const char* info_name, const char* temp_dir, int debug_on,
    NzbgetRsConnHostFn conn_host_cb, void* webdl_ctx, NzbgetRsWebDlHostFn webdl_cb,
    NzbgetRsBuf* out_buf);
int nzbget_rs_net_speedtest_iter(
    const char* body, size_t body_len, int duration_sec, int tls_disabled, int debug_on,
    NzbgetRsConnHostFn conn_host_cb, uint32_t* out_bytes, double* out_secs);
void nzbget_rs_req_proc_exec(
    NzbgetRsConn* conn, const NzbgetRsCancelReg* reg, int tls, int tls_disabled,
    int secure_port, int control_port, const char* secure_cert, const char* secure_key,
    void* proc_ctx, void (*serv_binrpc_cb)(void*),
    int (*serv_web_cb)(void*, const char*), void (*log_cb)(void*, int, const char*));
typedef struct NzbgetRsRemoteServer NzbgetRsRemoteServer;
typedef uintptr_t (*NzbgetRsRemoteSrvHostFn)(
    void* ctx, int op, int a1, uintptr_t p1, const char* s1,
    int* out_ints, const char** out_strs);
NzbgetRsRemoteServer* nzbget_rs_remote_srv_new(
    int tls, int tls_disabled, int debug_on, NzbgetRsConnHostFn conn_host_cb,
    void* srv_ctx, NzbgetRsRemoteSrvHostFn srv_cb);
void nzbget_rs_remote_srv_op(NzbgetRsRemoteServer* srv, int op, uintptr_t proc_ptr);

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

int nzbget_rs_script_config_exec(
    const NzbgetRsScriptConfig* sc, int op, int a1,
    const char* p1, size_t n1, const char* p2, size_t n2,
    void* ctx, int (*cancel_cb)(void*),
    NzbgetRsXmlRpcEmitFn emit_cb, NzbgetRsBuf* out_buf);
int nzbget_rs_web_serve(int op, const NzbgetRsWebServeEnv* env);
int nzbget_rs_statmeter_rpc(
    const NzbgetRsStatMeter* sm, int op, int local_time_offset,
    int a1, int a2, int a3, int a4, int a5,
    void* emit_ctx, NzbgetRsXmlRpcEmitFn emit_cb);

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

typedef struct NzbgetRsMaintenance NzbgetRsMaintenance;
typedef struct NzbgetRsMaintenanceEnv {
    void* ctx; uintptr_t p1; int a1; int a2; int a3; int a4; int64_t t1;
    const char* s1; const char* s2; const char* s3; NzbgetRsBuf* out_buf;
    void* emit_ctx; NzbgetRsXmlRpcEmitFn emit_cb; NzbgetRsScriptHostFn script_host_cb;
    uintptr_t (*action_cb)(void*, int, const char*, const char*, const uint8_t*, size_t);
} NzbgetRsMaintenanceEnv;
NzbgetRsMaintenance* nzbget_rs_maintenance_new(void);
void nzbget_rs_maintenance_free(NzbgetRsMaintenance* h);
int nzbget_rs_maintenance_exec(NzbgetRsMaintenance* h, int op, const NzbgetRsMaintenanceEnv* env);

typedef struct NzbgetRsDupeSearch NzbgetRsDupeSearch;
typedef uintptr_t (*NzbgetRsDupeHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, int64_t* i64s,
    const char** strs, const uint8_t* bytes, size_t len);
NzbgetRsDupeSearch* nzbget_rs_dupesearch_new(void);
void nzbget_rs_dupesearch_free(NzbgetRsDupeSearch* ptr);
void nzbget_rs_dupesearch_exec(
    NzbgetRsDupeSearch* ptr, int op, int a1, uintptr_t p1, uintptr_t p2,
    void* host_ctx, NzbgetRsDupeHostFn host_cb);
int nzbget_rs_fleet_append(
    void* host_ctx, NzbgetRsDupeHostFn host_cb,
    const char* dupe_key, const char* category, int priority, int timeout_sec,
    const NzbgetRsRpcFleetReqMember* members, size_t members_len,
    void* emit_ctx, NzbgetRsXmlRpcEmitFn emit_cb);

typedef struct NzbgetRsQueueCoordinator NzbgetRsQueueCoordinator;
typedef struct NzbgetRsPrePostProcessor NzbgetRsPrePostProcessor;
typedef uintptr_t (*NzbgetRsQueueSubsysHostFn)(
    void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);
int nzbget_rs_dupe_fallback_op(
    int op, int a1, uintptr_t p1, NzbgetRsQueueSubsysHostFn host_cb);
uintptr_t nzbget_rs_dupe_coord_exec(
    int op, int a1, uintptr_t p1, uintptr_t p2,
    const char* s1, const char* s2, NzbgetRsQueueSubsysHostFn host_cb);
NzbgetRsQueueCoordinator* nzbget_rs_queue_coord_new(void);
void nzbget_rs_queue_coord_free(NzbgetRsQueueCoordinator* ptr);
uintptr_t nzbget_rs_queue_coord_exec(
    const NzbgetRsQueueCoordinator* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, uintptr_t p3, const char* s1, const int* ints,
    NzbgetRsQueueSubsysHostFn host_cb, void* ctx);
NzbgetRsPrePostProcessor* nzbget_rs_prepost_new(uintptr_t owner);
void nzbget_rs_prepost_free(NzbgetRsPrePostProcessor* ptr);
uintptr_t nzbget_rs_prepost_exec(
    const NzbgetRsPrePostProcessor* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, uintptr_t p3, const char* s1,
    void* host_ctx, NzbgetRsQueueSubsysHostFn host_cb);
typedef struct NzbgetRsArticleDownloader NzbgetRsArticleDownloader;
NzbgetRsArticleDownloader* nzbget_rs_article_dl_new(
    int has_analyzer, int parcheck_enabled, uintptr_t initial_conn, const char* info_name);
void nzbget_rs_article_dl_free(NzbgetRsArticleDownloader* ptr);
uintptr_t nzbget_rs_article_dl_exec(
    NzbgetRsArticleDownloader* ptr, int op, int a1, int a2,
    uintptr_t p1, uintptr_t p2, const char* s1,
    NzbgetRsQueueSubsysHostFn host_cb, void* ctx);

#ifdef __cplusplus
}
#endif

#endif
