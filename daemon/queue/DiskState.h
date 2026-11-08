#ifndef DISKSTATE_H
#define DISKSTATE_H

#include <atomic>
#include <algorithm>
#include <functional>
#include <set>
#include <string>
#include <vector>
#include "Util.h"
#include "nzbget_rs.h"

#ifndef DOWNLOADINFO_H
#define DOWNLOADINFO_H
class NzbInfo;
class DownloadQueue;
class PostInfo;

class ServerStat
{
public:
	ServerStat(int serverId) : m_serverId(serverId) {}
	int GetServerId() { return m_serverId; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }

private:
	int m_serverId;
	int m_successArticles = 0;
	int m_failedArticles = 0;
};

typedef std::vector<ServerStat> ServerStatListBase;

class ServerStatList : public ServerStatListBase
{
public:
	enum EStatOperation
	{
		soSet,
		soAdd,
		soSubtract
	};

	void StatOp(int serverId, int successArticles, int failedArticles, EStatOperation statOperation);
	void ListOp(ServerStatList* serverStats, EStatOperation statOperation);
};

class SegmentData
{
public:
	virtual char* GetData() = 0;
	virtual ~SegmentData() {}
};

class ArticleInfo
{
public:
	enum EStatus
	{
		aiUndefined,
		aiRunning,
		aiFinished,
		aiFailed
	};

	void SetPartNumber(int s) { m_partNumber = s; }
	int GetPartNumber() { return m_partNumber; }
	const char* GetMessageId() { return m_messageId; }
	void SetMessageId(const char* messageId) { m_messageId = messageId; }
	void SetSize(int size) { m_size = size; }
	int GetSize() { return m_size; }
	void AttachSegment(std::unique_ptr<SegmentData> content, int64 offset, int size);
	void DiscardSegment();
	const char* GetSegmentContent() { return m_segmentContent ? m_segmentContent->GetData() : nullptr; }
	void SetSegmentOffset(int64 segmentOffset) { m_segmentOffset = segmentOffset; }
	int64 GetSegmentOffset() { return m_segmentOffset; }
	void SetSegmentSize(int segmentSize) { m_segmentSize = segmentSize; }
	int GetSegmentSize() { return m_segmentSize; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	const char* GetResultFilename() { return m_resultFilename; }
	void SetResultFilename(const char* resultFilename) { m_resultFilename = resultFilename; }
	uint32 GetCrc() { return m_crc; }
	void SetCrc(uint32 crc) { m_crc = crc; }
	int GetDupeFallbackRound() { return m_dupeFallbackRound; }
	void SetDupeFallbackRound(int dupeFallbackRound) { m_dupeFallbackRound = dupeFallbackRound; }
	const char* GetDupeOriginalMessageId() { return m_dupeOriginalMessageId; }
	void SetDupeOriginalMessageId(const char* messageId) { m_dupeOriginalMessageId = messageId; }
	int64 GetDupeExpectedOffset() { return m_dupeExpectedOffset; }
	void SetDupeExpectedOffset(int64 dupeExpectedOffset) { m_dupeExpectedOffset = dupeExpectedOffset; }
	int64 GetDupeExpectedEnd() { return m_dupeExpectedEnd; }
	void SetDupeExpectedEnd(int64 dupeExpectedEnd) { m_dupeExpectedEnd = dupeExpectedEnd; }
	std::vector<CString>* GetDupeSources() { return &m_dupeSources; }
	// the donor (nzb-id) behind each pinned source; the release's own message-id has none
	std::vector<int>* GetDupeSourceDonors() { return &m_dupeSourceDonors; }
	// the donor whose article filled this one (0: the release's own)
	int GetDupeDonorId() { return m_dupeDonorId; }
	void SetDupeDonorId(int dupeDonorId) { m_dupeDonorId = dupeDonorId; }
	int GetDupeLeadSnapshot() { return m_dupeLeadSnapshot; }
	void SetDupeLeadSnapshot(int dupeLeadSnapshot) { m_dupeLeadSnapshot = dupeLeadSnapshot; }
	int GetDupeNextLead() { return m_dupeNextLead; }
	void SetDupeNextLead(int dupeNextLead) { m_dupeNextLead = dupeNextLead; }
	int GetDupeDonorCount() { return m_dupeDonorCount; }
	void SetDupeDonorCount(int dupeDonorCount) { m_dupeDonorCount = dupeDonorCount; }
	bool GetDupeProactive() { return m_dupeProactive; }
	void SetDupeProactive(bool dupeProactive) { m_dupeProactive = dupeProactive; }

private:
	std::unique_ptr<SegmentData> m_segmentContent;
	std::atomic<int64> m_segmentOffset{0};
	std::atomic<int> m_segmentSize{0};
	std::atomic<uint32> m_crc{0};
	CString m_messageId;
	CString m_resultFilename;
	EStatus m_status = aiUndefined;
	int m_partNumber;
	int m_size = 0;
	// number of times this article was requeued with a message-id borrowed
	// from a duplicate collection (not persisted)
	int m_dupeFallbackRound = 0;
	// the article's own (primary) message-id, saved before the first
	// substitution so the primary can be tried as a revert source (not persisted)
	CString m_dupeOriginalMessageId;
	// decoded byte range a substituted (donor) article is expected to occupy,
	// derived from already-finished neighbour articles at substitution time;
	// -1 = unknown (not persisted)
	int64 m_dupeExpectedOffset = -1;
	int64 m_dupeExpectedEnd = -1;
	// the message-ids this article is tried against, pinned at its first
	// fallback so no later queue/history change or lead rotation can shift the
	// round->source mapping under the article (not persisted)
	std::vector<CString> m_dupeSources;
	std::vector<int> m_dupeSourceDonors;
	int m_dupeDonorId = 0;
	// the nzb-id of the donor whose article the pinned slot 0 fetches; a
	// lead-round result only counts towards demotion while this donor is still
	// the file's lead (not persisted)
	int m_dupeLeadSnapshot = 0;
	// the nzb-id of the next distinct donor in the pinned order - the donor a
	// demotion rotates the file's lead to; 0 = no other donor (not persisted)
	int m_dupeNextLead = 0;
	// distinct donors in the pinned order; bounds the lead rotation (not persisted)
	int m_dupeDonorCount = 0;
	// true when the first donor attempt was proactive cutover rather than a
	// retry after the target article failed on its primary source
	bool m_dupeProactive = false;
};

typedef std::vector<std::unique_ptr<ArticleInfo>> ArticleList;

class FileInfo
{
public:
	enum EPartialState
	{
		psNone,
		psPartial,
		psCompleted
	};

	typedef std::vector<CString> Groups;

	FileInfo(int id = 0) : m_id(id ? id : ++m_idGen) {}
	int GetId() { return m_id; }
	void SetId(int id);
	static void ResetGenId(bool max);
	static void RaiseIdFloor(int id) { m_idMax = std::max(m_idMax, id); }
	static int LastGenId() { return m_idGen; }
	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	ArticleList* GetArticles() { return &m_articles; }
	Groups* GetGroups() { return &m_groups; }
	const char* GetSubject() { return m_subject; }
	void SetSubject(const char* subject) { m_subject = subject; }
	const char* GetFilename() { return m_filename.c_str(); }
	void SetFilename(std::string filename) { m_filename = std::move(filename); }
	void SetFilename(const char* filename) { m_filename = filename ? filename : ""; }
	void SetOrigname(const char* origname) { m_origname = origname; }
	const char* GetOrigname() { return m_origname; }
	void MakeValidFilename();
	bool GetFilenameConfirmed() { return m_filenameConfirmed; }
	void SetFilenameConfirmed(bool filenameConfirmed) { m_filenameConfirmed = filenameConfirmed; }
	void SetSize(int64 size) { m_size = size; m_remainingSize = size; }
	int64 GetSize() { return m_size; }
	int64 GetRemainingSize() { return m_remainingSize; }
	void SetRemainingSize(int64 remainingSize) { m_remainingSize = remainingSize; }
	int64 GetMissedSize() { return m_missedSize; }
	void SetMissedSize(int64 missedSize) { m_missedSize = missedSize; }
	int64 GetSuccessSize() { return m_successSize; }
	void SetSuccessSize(int64 successSize) { m_successSize = successSize; }
	int64 GetFailedSize() { return m_failedSize; }
	void SetFailedSize(int64 failedSize) { m_failedSize = failedSize; }
	int GetTotalArticles() { return m_totalArticles; }
	void SetTotalArticles(int totalArticles) { m_totalArticles = totalArticles; }
	int GetMissedArticles() { return m_missedArticles; }
	void SetMissedArticles(int missedArticles) { m_missedArticles = missedArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetDupeRecoveredArticles() { return m_dupeRecoveredArticles; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { m_dupeRecoveredArticles = dupeRecoveredArticles; }
	int GetDupeAttemptedArticles() { return m_dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int dupeAttemptedArticles) { m_dupeAttemptedArticles = dupeAttemptedArticles; }
	bool GetDupeCutover() { return m_dupeCutover; }
	void SetDupeCutover(bool dupeCutover) { m_dupeCutover = dupeCutover; }
	// what the file's first article holds: borrowing from duplicates never writes
	// into par2 data, whatever the file is called
	enum EFirstContent
	{
		fcUnknown,
		fcPar2,
		fcOther
	};
	EFirstContent GetFirstContent() { return m_firstContent; }
	void SetFirstContent(EFirstContent firstContent) { m_firstContent = firstContent; }
	bool GetDupeNoSourceLogged() { return m_dupeNoSourceLogged; }
	void SetDupeNoSourceLogged(bool logged) { m_dupeNoSourceLogged = logged; }
	int GetDupeLeadDonorId() { return m_dupeLeadDonorId; }
	void SetDupeLeadDonorId(int dupeLeadDonorId) { m_dupeLeadDonorId = dupeLeadDonorId; }
	int GetDupeLeadFailures() { return m_dupeLeadFailures; }
	void SetDupeLeadFailures(int dupeLeadFailures) { m_dupeLeadFailures = dupeLeadFailures; }
	int GetDupeLeadSwitches() { return m_dupeLeadSwitches; }
	void SetDupeLeadSwitches(int dupeLeadSwitches) { m_dupeLeadSwitches = dupeLeadSwitches; }
	time_t GetTime() { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	bool GetPaused() { return m_paused; }
	void SetPaused(bool paused);
	bool GetDeleted() { return m_deleted; }
	void SetDeleted(bool Deleted) { m_deleted = Deleted; }
	int GetCompletedArticles() { return m_completedArticles; }
	void SetCompletedArticles(int completedArticles) { m_completedArticles = completedArticles; }
	bool GetParFile() { return m_parFile; }
	void SetParFile(bool parFile) { m_parFile = parFile; }
	Guard GuardOutputFile() { return Guard(m_outputFileMutex); }
	const std::string& GetOutputFilename() const { return m_outputFilename; }
	void SetOutputFilename(const char* outputFilename) { m_outputFilename = outputFilename ? outputFilename : ""; }
	void SetOutputFilename(std::string outputFilename) { m_outputFilename = std::move(outputFilename); }
	bool GetOutputInitialized() { return m_outputInitialized; }
	void SetOutputInitialized(bool outputInitialized) { m_outputInitialized = outputInitialized; }
	bool GetExtraPriority() { return m_extraPriority; }
	void SetExtraPriority(bool extraPriority);
	int GetActiveDownloads() { return m_activeDownloads; }
	void SetActiveDownloads(int activeDownloads);
	bool GetDupeDeleted() { return m_dupeDeleted; }
	void SetDupeDeleted(bool dupeDeleted) { m_dupeDeleted = dupeDeleted; }
	int GetCachedArticles() { return m_cachedArticles; }
	void SetCachedArticles(int cachedArticles) { m_cachedArticles = cachedArticles; }
	bool GetPartialChanged() { return m_partialChanged; }
	void SetPartialChanged(bool partialChanged) { m_partialChanged = partialChanged; }
	bool GetForceDirectWrite() { return m_forceDirectWrite; }
	void SetForceDirectWrite(bool forceDirectWrite) { m_forceDirectWrite = forceDirectWrite; }
	EPartialState GetPartialState() { return m_partialState; }
	void SetPartialState(EPartialState partialState) { m_partialState = partialState; }
	uint32 GetCrc() { return m_crc; }
	void SetCrc(uint32 crc) { m_crc = crc; }
	const char* GetHash16k() { return m_hash16k; }
	void SetHash16k(const char* hash16k) { m_hash16k = hash16k; }
	const char* GetParSetId() { return m_parSetId; }
	void SetParSetId(const char* parSetId) { m_parSetId = parSetId; }
	bool GetFlushLocked() { return m_flushLocked; }
	void SetFlushLocked(bool flushLocked) { m_flushLocked = flushLocked; }
	const std::string& GetHardLinkPath() const { return m_hardLinkPath; }
	void SetHardLinkPath(std::string hardLinkPath) { m_hardLinkPath = std::move(hardLinkPath); }
	bool IsHardLinked();
	int64 GetDecodedFileSize() { return m_decodedFileSize; }
	void SetDecodedFileSize(int64 decodedFileSize) { m_decodedFileSize = decodedFileSize; }

	ServerStatList* GetServerStats() { return &m_serverStats; }

private:
	int m_id;
	NzbInfo* m_nzbInfo = nullptr;
	ArticleList m_articles;
	Groups m_groups;
	ServerStatList m_serverStats;
	CString m_subject;
	std::string m_filename;
	CString m_origname;
	int64 m_size = 0;
	int64 m_remainingSize = 0;
	int64 m_successSize = 0;
	int64 m_failedSize = 0;
	int64 m_missedSize = 0;
	int m_totalArticles = 0;
	int m_missedArticles = 0;
	int m_failedArticles = 0;
	int m_successArticles = 0;
	// articles obtained from a duplicate collection via DupeArticleFallback (not persisted)
	int m_dupeRecoveredArticles = 0;
	// distinct articles for which a duplicate source was tried (not persisted);
	// the denominator for the recovered/attempted completion summary
	int m_dupeAttemptedArticles = 0;
	// once the primary proves to be missing many articles of this file, lead
	// with the duplicate for the remaining articles instead of failing on the
	// primary first (not persisted)
	bool m_dupeCutover = false;
	bool m_dupeNoSourceLogged = false;	// in memory only: said once per file
	EFirstContent m_firstContent = fcUnknown;	// in memory only: read from the file after a restart
	// nzb-id of the duplicate this file's fresh articles try first; rotated to
	// the next duplicate when the lead keeps missing articles, so fresh
	// articles do not re-fail on a holed posting; 0 = not decided yet, the
	// top-scored duplicate leads (not persisted)
	int m_dupeLeadDonorId = 0;
	// consecutive lead-donor misses; reaching LeadDemoteThreshold rotates
	// m_dupeLeadDonorId (not persisted)
	int m_dupeLeadFailures = 0;
	// lead rotations since the last lead success; capped at the donor count so
	// a release whose duplicates are ALL holed stops rotating (and logging)
	// once every duplicate has led (not persisted)
	int m_dupeLeadSwitches = 0;
	time_t m_time = 0;
	bool m_paused = false;
	bool m_deleted = false;
	bool m_filenameConfirmed = false;
	bool m_parFile = false;
	int m_completedArticles = 0;
	bool m_outputInitialized = false;
	std::string m_outputFilename;
	std::unique_ptr<Mutex> m_outputFileMutex;
	bool m_extraPriority = false;
	int m_activeDownloads = 0;
	bool m_dupeDeleted = false;
	std::atomic<int> m_cachedArticles{0};
	bool m_partialChanged = false;
	bool m_forceDirectWrite = false;
	EPartialState m_partialState = psNone;
	uint32 m_crc = 0;
	CString m_hash16k;
	CString m_parSetId;
	bool m_flushLocked = false;
	std::string m_hardLinkPath;
	// raw file size declared by the first successfully decoded yEnc article
	// (not persisted); used to verify articles borrowed from duplicates
	std::atomic<int64> m_decodedFileSize{0};

	static std::atomic<int> m_idGen;
	static int m_idMax;

	friend class CompletedFile;
};

typedef UniqueDeque<FileInfo> FileList;
typedef std::vector<FileInfo*> RawFileList;

class CompletedFile
{
public:
	enum EStatus
	{
		cfNone,
		cfSuccess,
		cfPartial,
		cfFailure
	};

	CompletedFile(int id, std::string filename, std::string oldname, EStatus status,
		uint32 crc, bool parFile, std::string hash16k, std::string parSetId);
	int GetId() { return m_id; }
	void SetFilename(std::string filename) { m_filename = std::move(filename); }
	const char* GetFilename() { return m_filename.c_str(); }
	void SetOrigname(std::string origname) { m_origname = std::move(origname); }
	const char* GetOrigname() { return m_origname.c_str(); }
	bool GetParFile() { return m_parFile; }
	EStatus GetStatus() { return m_status; }
	// a failed file recreated from a duplicate (stream repair) is partial:
	// its bytes are on disk, but were never download-verified
	void SetStatus(EStatus status) { m_status = status; }
	uint32 GetCrc() { return m_crc; }
	const char* GetHash16k() { return m_hash16k.c_str(); }
	void SetHash16k(std::string hash16k) { m_hash16k = std::move(hash16k); }
	const char* GetParSetId() { return m_parSetId.c_str(); }
	void SetParSetId(std::string parSetId) { m_parSetId = std::move(parSetId); }
	bool SameFilename(const char* name) const;

private:
	int m_id;
	EStatus m_status;
	uint32 m_crc;
	bool m_parFile;
	std::string m_filename;
	std::string m_origname;
	std::string m_hash16k;
	std::string m_parSetId;
};

typedef std::deque<CompletedFile> CompletedFileList;

// a contiguous byte range of a decoded file
struct StreamRange
{
	int64 Offset = 0;
	int64 Size = 0;
	int64 End() const { return Offset + Size; }
};

typedef std::vector<StreamRange> StreamRangeList;

/*
 * Byte ranges of a completed file which no news server could supply,
 * captured while the file's article list still exists, so post-processing
 * can repair them from a differently segmented duplicate posting
 * (see option <DupeArticleFallback> value "stream"; not persisted).
 */
class StreamRepairJob
{
public:
	StreamRepairJob(int fileId, const char* filename, int64 decodedFileSize, int64 failedSize,
		int64 missedSize, int failedArticles, bool parFile, StreamRangeList holes,
		uint64 stepsHash = 0) :
		m_fileId(fileId), m_filename(filename), m_decodedFileSize(decodedFileSize),
		m_failedSize(failedSize), m_missedSize(missedSize), m_failedArticles(failedArticles),
		m_parFile(parFile), m_holes(std::move(holes)), m_stepsHash(stepsHash) {}
	int GetFileId() const { return m_fileId; }
	const char* GetFilename() const { return m_filename; }
	int64 GetDecodedFileSize() const { return m_decodedFileSize; }
	// the file's ENCODED failed size (bytes= units, as counted into
	// m_currentFailedSize) and whether it is a par2 file, captured at
	// job-creation time while the FileInfo was still alive
	int64 GetFailedSize() const { return m_failedSize; }
	int64 GetMissedSize() const { return m_missedSize; }
	int GetFailedArticles() const { return m_failedArticles; }
	bool GetParFile() const { return m_parFile; }
	StreamRangeList* GetHoles() { return &m_holes; }
	const StreamRangeList* GetHoles() const { return &m_holes; }
	void SetHoles(StreamRangeList holes) { m_holes = std::move(holes); }
	// a whole-file job (0) learns its size from the first donor article
	void SetDecodedFileSize(int64 decodedFileSize) { m_decodedFileSize = decodedFileSize; }
	// article-size step fingerprint of the file's posting (see
	// DupeArticleFallback::ArticleSizeStepsHash), 0 = none
	uint64 GetStepsHash() const { return m_stepsHash; }
	bool GetLiveAttempted() const { return m_liveAttempted; }
	void SetLiveAttempted(bool liveAttempted) { m_liveAttempted = liveAttempted; }

private:
	int m_fileId;
	CString m_filename;
	int64 m_decodedFileSize;
	int64 m_failedSize;
	int64 m_missedSize;
	int m_failedArticles;
	bool m_parFile;
	StreamRangeList m_holes;
	uint64 m_stepsHash;
	// whether the download-concurrent live pass already tried this job once:
	// donors are static, so retrying live would only re-pay fetch traffic -
	// whatever is left waits for the post-processing pass (not persisted; a
	// restart re-allows one live attempt, which is harmless)
	bool m_liveAttempted = false;
};

typedef std::vector<StreamRepairJob> StreamRepairJobList;

class NzbParameter
{
public:
	NzbParameter(const char* name, const char* value) :
		m_name(name), m_value(value) {}
	const char* GetName() { return m_name; }
	const char* GetValue() { return m_value; }

private:
	CString m_name;
	CString m_value;

	void SetValue(const char* value) { m_value = value; }

	friend class NzbParameterList;
};

typedef std::deque<NzbParameter> NzbParameterListBase;

class NzbParameterList : public NzbParameterListBase
{
public:
	void SetParameter(const char* name, const char* value);
	NzbParameter* Find(const char* name);
	void CopyFrom(NzbParameterList* sourceParameters);
};

class ScriptStatus
{
public:
	enum EStatus
	{
		srNone,
		srFailure,
		srSuccess
	};

	ScriptStatus(const char* name, EStatus status) :
		m_name(name), m_status(status) {}
	const char* GetName() { return m_name; }
	EStatus GetStatus() { return m_status; }

private:
	CString m_name;
	EStatus m_status;

	friend class ScriptStatusList;
};

typedef std::deque<ScriptStatus> ScriptStatusListBase;

class ScriptStatusList : public ScriptStatusListBase
{
public:
	ScriptStatus::EStatus CalcTotalStatus();
};

#ifndef EDUPEMODE_DEFINED
#define EDUPEMODE_DEFINED
enum EDupeMode
{
	dmScore,
	dmAll,
	dmForce
};
#endif

class NzbInfo
{
public:
	enum EDirectRenameStatus
	{
		tsNone,
		tsRunning,
		tsFailure,
		tsSuccess
	};

	enum EPostRenameStatus
	{
		rsNone,
		rsSkipped,
		rsNothing,
		rsSuccess
	};

	enum EParStatus
	{
		psNone,
		psSkipped,
		psFailure,
		psSuccess,
		psRepairPossible,
		psManual
	};

	enum EDirectUnpackStatus
	{
		nsNone,
		nsRunning,
		nsFailure,
		nsSuccess
	};

	enum EPostUnpackStatus
	{
		usNone,
		usSkipped,
		usFailure,
		usSuccess,
		usSpace,
		usPassword
	};

	enum ECleanupStatus
	{
		csNone,
		csFailure,
		csSuccess
	};

	enum EMoveStatus
	{
		msNone,
		msFailure,
		msSuccess
	};

	enum class PostUnpackRenamingStatus
	{
		None,
		Failure,
		Success,
		Skipped
	};

	enum class PostDownloadRenamingStatus
	{
		None,
		Failure,
		Success,
		Skipped
	};

	enum EDeleteStatus
	{
		dsNone,
		dsManual,
		dsHealth,
		dsDupe,
		dsBad,
		dsGood,
		dsCopy,
		dsScan
	};

	enum EMarkStatus
	{
		ksNone,
		ksBad,
		ksGood,
		ksSuccess
	};

	enum EUrlStatus
	{
		lsNone,
		lsRunning,
		lsFinished,
		lsFailed,
		lsRetry,
		lsScanSkipped,
		lsScanFailed
	};

	enum EKind
	{
		nkNzb,
		nkUrl
	};

	enum EDupeHint
	{
		dhNone,
		dhRedownloadManual,
		dhRedownloadAuto
	};

	int GetId() { return m_id; }
	void SetId(int id);
	static void ResetGenId(bool max);
	static void RaiseIdFloor(int id) { m_idMax = std::max(m_idMax, id); }
	static int LastGenId() { return m_idGen; }
	static int GenerateId();
	EKind GetKind() { return m_kind; }
	void SetKind(EKind kind) { m_kind = kind; }
	const char* GetUrl() { return m_url; }
	void SetUrl(const char* url);
	const char* GetFilename() { return m_filename; }
	void SetFilename(const char* filename);
	static CString MakeNiceNzbName(const char* nzbFilename, bool removeExt);
	static CString MakeNiceUrlName(const char* url, const char* nzbFilename);
	const char* GetDestDir() { return m_destDir; }
	void SetDestDir(const char* destDir) { m_destDir = destDir; }
	const char* GetFinalDir() { return m_finalDir; }
	void SetFinalDir(const char* finalDir) { m_finalDir = finalDir; }
	const char* GetCategory() { return m_category; }
	void SetCategory(const char* category) { m_category = category; }
	const char* GetName() { return m_name; }
	void SetHardLinkPath(std::string hardLinkPath) { m_hardLinkPath = std::move(hardLinkPath); }
	const std::string& GetHardLinkPath() const { return m_hardLinkPath; }
	void SetName(const char* name) { m_name = name; }
	const std::string& GetMetaName() const { return m_metaName; }
	void SetMetaName(std::string metaName) { m_metaName = std::move(metaName); }
	int GetFileCount() { return m_fileCount; }
	void SetFileCount(int fileCount) { m_fileCount = fileCount; }
	int GetParkedFileCount() { return m_parkedFileCount; }
	void SetParkedFileCount(int parkedFileCount) { m_parkedFileCount = parkedFileCount; }
	int64 GetSize() { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	int64 GetRemainingSize() { return m_remainingSize; }
	void SetRemainingSize(int64 remainingSize) { m_remainingSize = remainingSize; }
	int64 GetPausedSize() { return m_pausedSize; }
	void SetPausedSize(int64 pausedSize) { m_pausedSize = pausedSize; }
	int GetPausedFileCount() { return m_pausedFileCount; }
	void SetPausedFileCount(int pausedFileCount) { m_pausedFileCount = pausedFileCount; }
	int GetRemainingParCount() { return m_remainingParCount; }
	void SetRemainingParCount(int remainingParCount) { m_remainingParCount = remainingParCount; }
	int GetActiveDownloads() { return m_activeDownloads; }
	void SetActiveDownloads(int activeDownloads);
	int64 GetSuccessSize() { return m_successSize; }
	void SetSuccessSize(int64 successSize) { m_successSize = successSize; }
	int64 GetFailedSize() { return m_failedSize; }
	void SetFailedSize(int64 failedSize) { m_failedSize = failedSize; }
	int64 GetCurrentSuccessSize() { return m_currentSuccessSize; }
	void SetCurrentSuccessSize(int64 currentSuccessSize) { m_currentSuccessSize = currentSuccessSize; }
	int64 GetCurrentFailedSize() { return m_currentFailedSize; }
	void SetCurrentFailedSize(int64 currentFailedSize) { m_currentFailedSize = currentFailedSize; }
	int64 GetParSize() { return m_parSize; }
	void SetParSize(int64 parSize) { m_parSize = parSize; }
	int64 GetParSuccessSize() { return m_parSuccessSize; }
	void SetParSuccessSize(int64 parSuccessSize) { m_parSuccessSize = parSuccessSize; }
	int64 GetParFailedSize() { return m_parFailedSize; }
	void SetParFailedSize(int64 parFailedSize) { m_parFailedSize = parFailedSize; }
	int64 GetParCurrentSuccessSize() { return m_parCurrentSuccessSize; }
	void SetParCurrentSuccessSize(int64 parCurrentSuccessSize) { m_parCurrentSuccessSize = parCurrentSuccessSize; }
	int64 GetParCurrentFailedSize() { return m_parCurrentFailedSize; }
	void SetParCurrentFailedSize(int64 parCurrentFailedSize) { m_parCurrentFailedSize = parCurrentFailedSize; }
	int GetTotalArticles() { return m_totalArticles; }
	void SetTotalArticles(int totalArticles) { m_totalArticles = totalArticles; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }
	int GetCurrentSuccessArticles() { return m_currentSuccessArticles; }
	void SetCurrentSuccessArticles(int currentSuccessArticles) { m_currentSuccessArticles = currentSuccessArticles; }
	int GetCurrentFailedArticles() { return m_currentFailedArticles; }
	void SetCurrentFailedArticles(int currentFailedArticles) { m_currentFailedArticles = currentFailedArticles; }
	int GetDupeRecoveredArticles() { return m_dupeRecoveredArticles; }
	// duplicates whose articles failed the par2 checksums: no more articles from them (B40, not persisted)
	std::set<int>* GetDupeBlockedDonors() { return &m_dupeBlockedDonors; }
	// failed articles of its own in a row, first articles of files aside (B49, not persisted)
	int GetDupeFailedRun() { return m_dupeFailedRun; }
	void SetDupeFailedRun(int dupeFailedRun) { m_dupeFailedRun = dupeFailedRun; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { m_dupeRecoveredArticles = dupeRecoveredArticles; }
	int64 GetDupeRecoveredBytes() { return m_dupeRecoveredBytes; }
	void SetDupeRecoveredBytes(int64 value) { m_dupeRecoveredBytes = value; }
	int GetDupeRecoveredHoles() { return m_dupeRecoveredHoles; }
	void SetDupeRecoveredHoles(int value) { m_dupeRecoveredHoles = value; }
	int GetDupeAttemptedArticles() { return m_dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int value) { m_dupeAttemptedArticles = value; }
	int GetDupeUnsourcedArticles() { return m_dupeUnsourcedArticles; }
	void SetDupeUnsourcedArticles(int value) { m_dupeUnsourcedArticles = value; }
	int GetDupeFailoverChecked() { return m_dupeFailoverChecked; }
	void SetDupeFailoverChecked(int value) { m_dupeFailoverChecked = value; }
	bool GetDeadPickProbed() { return m_deadPickProbed; }
	void SetDeadPickProbed(bool value) { m_deadPickProbed = value; }
	enum EDupeParDeferState { dpNone, dpDeferred, dpLifted };
	EDupeParDeferState GetDupeParDeferState() { return m_dupeParDeferState; }
	void SetDupeParDeferState(EDupeParDeferState state) { m_dupeParDeferState = state; }
	bool GetDupeNoParNoted() { return m_dupeNoParNoted; }
	void SetDupeNoParNoted(bool noted) { m_dupeNoParNoted = noted; }
	StreamRepairJobList* GetStreamRepairJobs() { return &m_streamRepairJobs; }
	int GetPriority() { return m_priority; }
	void SetPriority(int priority) { m_priority = priority; }
	int GetExtraPriority() { return m_extraPriority; }
	void SetExtraPriority(int extraPriority) { m_extraPriority = extraPriority; }
	bool HasExtraPriority() { return m_extraPriority > 0; }
	bool GetForcePriority() { return m_priority >= FORCE_PRIORITY; }
	time_t GetMinTime() { return m_minTime; }
	void SetMinTime(time_t minTime) { m_minTime = minTime; }
	time_t GetMaxTime() { return m_maxTime; }
	void SetMaxTime(time_t maxTime) { m_maxTime = maxTime; }
	void BuildDestDirName();
	CString BuildFinalDirName();
	CompletedFileList* GetCompletedFiles() { return &m_completedFiles; }
	bool RenameCompletedFile(const char* oldName, const char* newName);
	void SetDirectRenameStatus(EDirectRenameStatus renameStatus) { m_directRenameStatus = renameStatus; }
	EDirectRenameStatus GetDirectRenameStatus() { return m_directRenameStatus; }
	EPostRenameStatus GetParRenameStatus() { return m_parRenameStatus; }
	void SetParRenameStatus(EPostRenameStatus renameStatus) { m_parRenameStatus = renameStatus; }
	EPostRenameStatus GetRarRenameStatus() { return m_rarRenameStatus; }
	void SetRarRenameStatus(EPostRenameStatus renameStatus) { m_rarRenameStatus = renameStatus; }
	EParStatus GetParStatus() { return m_parStatus; }
	void SetParStatus(EParStatus parStatus) { m_parStatus = parStatus; }
	EDirectUnpackStatus GetDirectUnpackStatus() { return m_directUnpackStatus; }
	void SetDirectUnpackStatus(EDirectUnpackStatus directUnpackStatus) { m_directUnpackStatus = directUnpackStatus; }
	EPostUnpackStatus GetUnpackStatus() { return m_unpackStatus; }
	void SetUnpackStatus(EPostUnpackStatus unpackStatus) { m_unpackStatus = unpackStatus; }
	ECleanupStatus GetCleanupStatus() { return m_cleanupStatus; }
	void SetCleanupStatus(ECleanupStatus cleanupStatus) { m_cleanupStatus = cleanupStatus; }
	EMoveStatus GetMoveStatus() { return m_moveStatus; }
	void SetPostUnpackRenamingStatus(PostUnpackRenamingStatus status) { m_postUnpackRenamingStatus = status; }
	PostUnpackRenamingStatus GetPostUnpackRenamingStatus() { return m_postUnpackRenamingStatus; }
	void SetPostDownloadRenamingStatus(PostDownloadRenamingStatus status) { m_postDownloadRenamingStatus = status; }
	PostDownloadRenamingStatus GetPostDownloadRenamingStatus() { return m_postDownloadRenamingStatus; }
	void SetMoveStatus(EMoveStatus moveStatus) { m_moveStatus = moveStatus; }
	EDeleteStatus GetDeleteStatus() { return m_deleteStatus; }
	void SetDeleteStatus(EDeleteStatus deleteStatus) { m_deleteStatus = deleteStatus; }
	EMarkStatus GetMarkStatus() { return m_markStatus; }
	void SetMarkStatus(EMarkStatus markStatus) { m_markStatus = markStatus; }
	EUrlStatus GetUrlStatus() { return m_urlStatus; }
	int GetExtraParBlocks() { return m_extraParBlocks; }
	void SetExtraParBlocks(int extraParBlocks) { m_extraParBlocks = extraParBlocks; }
	void SetUrlStatus(EUrlStatus urlStatus) { m_urlStatus = urlStatus; }
	const char* GetQueuedFilename() { return m_queuedFilename; }
	void SetQueuedFilename(const char* queuedFilename) { m_queuedFilename = queuedFilename; }
	bool GetDeleting() { return m_deleting; }
	void SetDeleting(bool deleting) { m_deleting = deleting; }
	bool GetParking() { return m_parking; }
	void SetParking(bool parking) { m_parking = parking; }
	bool GetDeletePaused() { return m_deletePaused; }
	void SetDeletePaused(bool deletePaused) { m_deletePaused = deletePaused; }
	bool GetManyDupeFiles() { return m_manyDupeFiles; }
	void SetManyDupeFiles(bool manyDupeFiles) { m_manyDupeFiles = manyDupeFiles; }
	bool GetAvoidHistory() { return m_avoidHistory; }
	void SetAvoidHistory(bool avoidHistory) { m_avoidHistory = avoidHistory; }
	bool GetHealthPaused() { return m_healthPaused; }
	void SetHealthPaused(bool healthPaused) { m_healthPaused = healthPaused; }
	bool GetCleanupDisk() { return m_cleanupDisk; }
	void SetCleanupDisk(bool cleanupDisk) { m_cleanupDisk = cleanupDisk; }
	bool GetUnpackCleanedUpDisk() { return m_unpackCleanedUpDisk; }
	void SetUnpackCleanedUpDisk(bool unpackCleanedUpDisk) { m_unpackCleanedUpDisk = unpackCleanedUpDisk; }
	bool GetAddUrlPaused() { return m_addUrlPaused; }
	void SetAddUrlPaused(bool addUrlPaused) { m_addUrlPaused = addUrlPaused; }
	FileList* GetFileList() { return &m_fileList; }
	NzbParameterList* GetParameters() { return &m_ppParameters; }
	ScriptStatusList* GetScriptStatuses() { return &m_scriptStatuses; }
	ServerStatList* GetServerStats() { return &m_serverStats; }
	ServerStatList* GetCurrentServerStats() { return &m_currentServerStats; }
	int CalcHealth();
	int CalcCriticalHealth(bool allowEstimation);
	const char* GetDupeKey() { return m_dupeKey; }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey ? dupeKey : ""; }
	int GetDupeScore() { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	EDupeHint GetDupeHint() { return m_dupeHint; }
	void SetDupeHint(EDupeHint dupeHint) { m_dupeHint = dupeHint; }
	uint32 GetFullContentHash() { return m_fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { m_fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return m_filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { m_filteredContentHash = filteredContentHash; }
	int64 GetDownloadedSize() { return m_downloadedSize; }
	void SetDownloadedSize(int64 downloadedSize) { m_downloadedSize = downloadedSize; }
	int GetDownloadSec() { return m_downloadSec; }
	void SetDownloadSec(int downloadSec) { m_downloadSec = downloadSec; }
	int GetPostTotalSec() { return m_postTotalSec; }
	void SetPostTotalSec(int postTotalSec) { m_postTotalSec = postTotalSec; }
	int GetParSec() { return m_parSec; }
	void SetParSec(int parSec) { m_parSec = parSec; }
	int GetRepairSec() { return m_repairSec; }
	void SetRepairSec(int repairSec) { m_repairSec = repairSec; }
	int GetUnpackSec() { return m_unpackSec; }
	void SetUnpackSec(int unpackSec) { m_unpackSec = unpackSec; }
	time_t GetDownloadStartTime() { return m_downloadStartTime; }
	void SetDownloadStartTime(time_t downloadStartTime) { m_downloadStartTime = downloadStartTime; }
	bool GetChanged() { return m_changed; }
	void SetChanged(bool changed) { m_changed = changed; }
	void SetReprocess(bool reprocess) { m_reprocess = reprocess; }
	bool GetReprocess() { return m_reprocess; }
	time_t GetQueueScriptTime() { return m_queueScriptTime; }
	void SetQueueScriptTime(time_t queueScriptTime) { m_queueScriptTime = queueScriptTime; }
	void SetParFull(bool parFull) { m_parFull = parFull; }
	bool GetParFull() { return m_parFull; }
	int GetFeedId() { return m_feedId; }
	void SetFeedId(int feedId) { m_feedId = feedId; }
	void MoveFileList(NzbInfo* srcNzbInfo);
	void UpdateMinMaxTime();
	PostInfo* GetPostInfo() { return m_postInfo.get(); }
	bool GetParRepaired() { return m_parRepaired; }
	void SetParRepaired(bool parRepaired) { m_parRepaired = parRepaired; }
	bool GetUnpackTried() { return m_unpackTried; }
	void SetUnpackTried(bool unpackTried) { m_unpackTried = unpackTried; }
	int GetLastUnpackStatus() { return m_lastUnpackStatus; }
	void SetLastUnpackStatus(int lastUnpackStatus) { m_lastUnpackStatus = lastUnpackStatus; }
	std::vector<CString>* GetExtractedArchives() { return &m_extractedArchives; }
	void EnterPostProcess();
	void LeavePostProcess();
	bool IsDupeSuccess();
	const char* MakeTextStatus(bool ignoreScriptStatus);
	void AddMessage(Message::EKind kind, const char* text, bool print = true);
	void PrintMessage(Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(3);
	int GetMessageCount() const { return m_messageCount; }
	void SetMessageCount(int messageCount) { m_messageCount = messageCount; }
	int GetCachedMessageCount() { return m_cachedMessageCount; }
	GuardedMessageList GuardCachedMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }
	bool GetAllFirst() { return m_allFirst; }
	void SetAllFirst(bool allFirst) { m_allFirst = allFirst; }
	bool GetWaitingPar() { return m_waitingPar; }
	void SetWaitingPar(bool waitingPar) { m_waitingPar = waitingPar; }
	bool GetLoadingPar() { return m_loadingPar; }
	void SetLoadingPar(bool loadingPar) { m_loadingPar = loadingPar; }
	Thread* GetUnpackThread() { return m_unpackThread; }
	void SetUnpackThread(Thread* unpackThread) { m_unpackThread = unpackThread; }
	Thread* GetLiveRepairThread() { return m_liveRepairThread; }
	void SetLiveRepairThread(Thread* liveRepairThread) { m_liveRepairThread = liveRepairThread; }
	void UpdateCurrentStats();
	void UpdateCompletedStats(FileInfo* fileInfo);
	void UpdateDeletedStats(FileInfo* fileInfo);
	bool IsDownloadCompleted(bool ignorePausedPars);
	void SetDesiredServerId(int id) { m_desiredServerId = id; }
	int GetDesiredServerId() { return m_desiredServerId; }
	bool GetSkipScriptProcessing() { return m_skipScriptProcessing; }
	void SetSkipScriptProcessing(bool skip) { m_skipScriptProcessing = skip; }
	void SetSkipDiskWrite(bool skipWrite) { m_skipDiskWrite = skipWrite; }
	bool GetSkipDiskWrite() { return m_skipDiskWrite; }
	void SetAutoCategory(bool autoCategory) { m_autoCategory = autoCategory; }
	bool GetAutoCategory() const { return m_autoCategory; }
	bool HasDesiredServer() const { return m_desiredServerId > 0; }

	static const int FORCE_PRIORITY = 900;

private:
	int m_id = ++m_idGen;
	EKind m_kind = nkNzb;
	CString m_url = "";
	CString m_filename = "";
	CString m_name;
	CString m_destDir = "";
	CString m_finalDir = "";
	CString m_category = "";
	std::string m_hardLinkPath;
	std::string m_metaName;
	int m_fileCount = 0;
	int m_parkedFileCount = 0;
	int64 m_size = 0;
	int64 m_remainingSize = 0;
	int m_pausedFileCount = 0;
	int64 m_pausedSize = 0;
	int m_remainingParCount = 0;
	int m_activeDownloads = 0;
	int64 m_successSize = 0;
	int64 m_failedSize = 0;
	int64 m_currentSuccessSize = 0;
	int64 m_currentFailedSize = 0;
	int64 m_parSize = 0;
	int64 m_parSuccessSize = 0;
	int64 m_parFailedSize = 0;
	int64 m_parCurrentSuccessSize = 0;
	int64 m_parCurrentFailedSize = 0;
	int m_totalArticles = 0;
	int m_successArticles = 0;
	int m_failedArticles = 0;
	int m_currentSuccessArticles = 0;
	int m_currentFailedArticles = 0;
	// aggregate of FileInfo::m_dupeRecoveredArticles (persisted for restart)
	int m_dupeRecoveredArticles = 0;
	std::set<int> m_dupeBlockedDonors;
	int m_dupeFailedRun = 0;
	int64 m_dupeRecoveredBytes = 0;
	int m_dupeRecoveredHoles = 0;
	// failed articles that were tried from duplicates / that no duplicate
	// could even offer a source for (the early dupe failover's sample of how
	// the download-time recovery is faring; not persisted)
	int m_dupeAttemptedArticles = 0;
	int m_dupeUnsourcedArticles = 0;
	// failed-article count at the last history scan of the early dupe failover
	// (-1 = never scanned), so a download without a backup is not rescanned
	// for every failed article
	int m_dupeFailoverChecked = -1;
	bool m_deadPickProbed = false;
	// which way the par-first rule of duplicate recovery went, for logging once (not persisted)
	EDupeParDeferState m_dupeParDeferState = dpNone;
	bool m_dupeNoParNoted = false;	// "no par2: not borrowing" logged (not saved)
	// missing byte ranges of completed media files awaiting stream repair
	// from duplicate collections in post-processing (persisted)
	StreamRepairJobList m_streamRepairJobs;
	time_t m_minTime = 0;
	time_t m_maxTime = 0;
	int m_priority = 0;
	int m_extraPriority = 0;
	CompletedFileList m_completedFiles;
	EDirectRenameStatus m_directRenameStatus = tsNone;
	EPostRenameStatus m_parRenameStatus = rsNone;
	EPostRenameStatus m_rarRenameStatus = rsNone;
	EParStatus m_parStatus = psNone;
	EDirectUnpackStatus m_directUnpackStatus = nsNone;
	EPostUnpackStatus m_unpackStatus = usNone;
	ECleanupStatus m_cleanupStatus = csNone;
	EMoveStatus m_moveStatus = msNone;
	PostUnpackRenamingStatus m_postUnpackRenamingStatus = PostUnpackRenamingStatus::None;
	PostDownloadRenamingStatus m_postDownloadRenamingStatus = PostDownloadRenamingStatus::None;
	EDeleteStatus m_deleteStatus = dsNone;
	EMarkStatus m_markStatus = ksNone;
	EUrlStatus m_urlStatus = lsNone;
	int m_extraParBlocks = 0;
	bool m_addUrlPaused = false;
	bool m_deletePaused = false;
	bool m_manyDupeFiles = false;
	CString m_queuedFilename = "";
	bool m_deleting = false;
	bool m_parking = false;
	bool m_avoidHistory = false;
	bool m_healthPaused = false;
	bool m_parManual = false;
	bool m_cleanupDisk = false;
	bool m_unpackCleanedUpDisk = false;
	CString m_dupeKey = "";
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	EDupeHint m_dupeHint = dhNone;
	uint32 m_fullContentHash = 0;
	uint32 m_filteredContentHash = 0;
	FileList m_fileList;
	NzbParameterList m_ppParameters;
	ScriptStatusList m_scriptStatuses;
	ServerStatList m_serverStats;
	ServerStatList m_currentServerStats;
	Mutex m_logMutex;
	MessageList m_messages;
	int m_idMessageGen = 0;
	std::unique_ptr<PostInfo> m_postInfo;
	bool m_parRepaired = false;
	bool m_unpackTried = false;
	int m_lastUnpackStatus = 0;
	std::vector<CString> m_extractedArchives;
	int64 m_downloadedSize = 0;
	time_t m_downloadStartTime = 0;
	int m_downloadStartSec = 0;
	int m_downloadSec = 0;
	int m_postTotalSec = 0;
	int m_parSec = 0;
	int m_repairSec = 0;
	int m_unpackSec = 0;
	bool m_reprocess = false;
	bool m_changed = false;
	time_t m_queueScriptTime = 0;
	bool m_parFull = false;
	std::atomic<int> m_messageCount{0};
	int m_cachedMessageCount = 0;
	int m_feedId = 0;
	bool m_allFirst = false;
	bool m_waitingPar = false;
	bool m_loadingPar = false;
	Thread* m_unpackThread = nullptr;
	// download-concurrent stream repair for completed-with-holes files (see
	// option <DupeArticleFallback> value "live"; not persisted)
	Thread* m_liveRepairThread = nullptr;
	int m_desiredServerId = 0;
	bool m_skipScriptProcessing = false;
	bool m_skipDiskWrite = false;
	bool m_autoCategory = false;

	static std::atomic<int> m_idGen;
	static int m_idMax;

	void ClearMessages();

	friend class DupInfo;
};

typedef UniqueDeque<NzbInfo> NzbList;
typedef std::vector<NzbInfo*> RawNzbList;

class PostInfo
{
public:
	enum EStage
	{
		ptQueued,
		ptLoadingPars,
		ptVerifyingSources,
		ptRepairing,
		ptVerifyingRepaired,
		ptParRenaming,
		ptRarRenaming,
		ptUnpacking,
		ptCleaningUp,
		ptMoving,
		ptPostUnpackRenaming,
		ptExecutingScript,
		ptFinished,
		// appended after ptFinished: stage numbers are persisted (stage+1 in
		// the queue file) and range checks rely on the existing order
		ptPostDownloadRenaming,
		ptStreamRepairing
	};

	typedef std::vector<CString> ParredFiles;
	typedef std::vector<CString> ExtractedArchives;

	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	EStage GetStage() { return m_stage; }
	void SetStage(EStage stage) { m_stage = stage; }
	void SetProgressLabel(const char* progressLabel) { m_progressLabel = progressLabel; }
	const char* GetProgressLabel() { return m_progressLabel; }
	int GetFileProgress() { return m_fileProgress; }
	void SetFileProgress(int fileProgress) { m_fileProgress = fileProgress; }
	int GetStageProgress() { return m_stageProgress; }
	void SetStageProgress(int stageProgress) { m_stageProgress = std::clamp(stageProgress, 0, 1000); }
	time_t GetStartTime() { return m_startTime; }
	void SetStartTime(time_t startTime) { m_startTime = startTime; }
	time_t GetStageTime() { return m_stageTime; }
	void SetStageTime(time_t stageTime) { m_stageTime = stageTime; }
	bool GetWorking() { return m_working; }
	void SetWorking(bool working) { m_working = working; }
	bool GetDeleted() { return m_deleted; }
	void SetDeleted(bool deleted) { m_deleted = deleted; }
	bool GetRequestParCheck() { return m_requestParCheck; }
	void SetRequestParCheck(bool requestParCheck) { m_requestParCheck = requestParCheck; }
	bool GetForceParFull() { return m_forceParFull; }
	void SetForceParFull(bool forceParFull) { m_forceParFull = forceParFull; }
	bool GetStreamRepairDone() { return m_streamRepairDone; }
	void SetStreamRepairDone(bool streamRepairDone) { m_streamRepairDone = streamRepairDone; }
	bool GetStreamRepairRecovered() { return m_streamRepairRecovered; }
	void SetStreamRepairRecovered(bool streamRepairRecovered) { m_streamRepairRecovered = streamRepairRecovered; }
	bool GetParCannotCover() { return m_parCannotCover; }
	void SetParCannotCover(bool parCannotCover) { m_parCannotCover = parCannotCover; }
	bool GetForceRepair() { return m_forceRepair; }
	void SetForceRepair(bool forceRepair) { m_forceRepair = forceRepair; }
	bool GetParRepaired() { return m_nzbInfo ? m_nzbInfo->GetParRepaired() : m_parRepaired; }
	void SetParRepaired(bool parRepaired)
	{
		m_parRepaired = parRepaired;
		if (m_nzbInfo) m_nzbInfo->SetParRepaired(parRepaired);
	}
	bool GetUnpackTried() { return m_nzbInfo ? m_nzbInfo->GetUnpackTried() : m_unpackTried; }
	void SetUnpackTried(bool unpackTried)
	{
		m_unpackTried = unpackTried;
		if (m_nzbInfo) m_nzbInfo->SetUnpackTried(unpackTried);
	}
	bool GetPassListTried() { return m_passListTried; }
	void SetPassListTried(bool passListTried) { m_passListTried = passListTried; }
	int GetLastUnpackStatus() { return m_nzbInfo ? m_nzbInfo->GetLastUnpackStatus() : m_lastUnpackStatus; }
	void SetLastUnpackStatus(int unpackStatus)
	{
		m_lastUnpackStatus = unpackStatus;
		if (m_nzbInfo) m_nzbInfo->SetLastUnpackStatus(unpackStatus);
	}
	bool GetNeedParCheck() { return m_needParCheck; }
	void SetNeedParCheck(bool needParCheck) { m_needParCheck = needParCheck; }
	bool GetNeedUnpack() const { return m_needUnpack; }
	void SetNeedUnpack(bool needUnpack) { m_needUnpack = needUnpack; }
	Thread* GetPostThread() { return m_postThread; }
	void SetPostThread(Thread* postThread) { m_postThread = postThread; }
	ParredFiles* GetParredFiles() { return &m_parredFiles; }
	ExtractedArchives* GetExtractedArchives() { return m_nzbInfo ? m_nzbInfo->GetExtractedArchives() : &m_extractedArchives; }

private:
	NzbInfo* m_nzbInfo = nullptr;
	std::atomic<bool> m_working{false};
	bool m_deleted = false;
	bool m_requestParCheck = false;
	bool m_forceParFull = false;
	// the post-processing stream repair has run, and whether it wrote bytes
	bool m_streamRepairDone = false;
	bool m_streamRepairRecovered = false;
	// par-check was postponed because the par2 files cannot cover the damage
	bool m_parCannotCover = false;
	bool m_forceRepair = false;
	bool m_parRepaired = false;
	bool m_unpackTried = false;
	bool m_passListTried = false;
	int m_lastUnpackStatus = 0;
	bool m_needParCheck = false;
	bool m_needUnpack = false;
	EStage m_stage = ptQueued;
	CString m_progressLabel = "";
	int m_fileProgress = 0;
	std::atomic<int> m_stageProgress{0};
	time_t m_startTime = 0;
	time_t m_stageTime = 0;
	Thread* m_postThread = nullptr;
	ParredFiles m_parredFiles;
	ExtractedArchives m_extractedArchives;
};

typedef std::vector<int> IdList;

typedef std::vector<CString> NameList;

class DupInfo
{
public:
	enum EStatus
	{
		dsUndefined,
		dsSuccess,
		dsFailed,
		dsDeleted,
		dsDupe,
		dsBad,
		dsGood
	};

	int GetId() { return m_id; }
	void SetId(int id);
	const char* GetName() { return m_name; }
	void SetName(const char* name) { m_name = name; }
	const char* GetDupeKey() { return m_dupeKey; }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey; }
	int GetDupeScore() { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	int64 GetSize() { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	uint32 GetFullContentHash() { return m_fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { m_fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return m_filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { m_filteredContentHash = filteredContentHash; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }

private:
	int m_id = 0;
	CString m_name;
	CString m_dupeKey;
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	int64 m_size = 0;
	uint32 m_fullContentHash = 0;
	uint32 m_filteredContentHash = 0;
	EStatus m_status = dsUndefined;
};

class HistoryInfo
{
public:
	enum EKind
	{
		hkUnknown,
		hkNzb,
		hkUrl,
		hkDup
	};

	HistoryInfo(std::unique_ptr<NzbInfo> nzbInfo) : m_info(nzbInfo.release()),
		m_kind(GetNzbInfo()->GetKind() == NzbInfo::nkNzb ? hkNzb : hkUrl) {}
	HistoryInfo(std::unique_ptr<DupInfo> dupInfo) : m_info(dupInfo.release()), m_kind(hkDup) {}
	~HistoryInfo();
	EKind GetKind() { return m_kind; }
	int GetId();
	NzbInfo* GetNzbInfo() { return (NzbInfo*)m_info; }
	DupInfo* GetDupInfo() { return (DupInfo*)m_info; }
	void DiscardNzbInfo() { m_info = nullptr; }
	time_t GetTime() { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	const char* GetName();

private:
	void* m_info;
	EKind m_kind;
	time_t m_time = 0;
};

typedef UniqueDeque<HistoryInfo> HistoryList;

typedef GuardedPtr<DownloadQueue> GuardedDownloadQueue;

class DownloadQueue : public Subject
{
public:
	enum EAspectAction
	{
		eaNzbFound,
		eaNzbAdded,
		eaNzbDeleted,
		eaNzbNamed,
		eaNzbReturned,
		eaFileCompleted,
		eaFileDeleted,
		eaUrlFound,
		eaUrlAdded,
		eaUrlDeleted,
		eaUrlCompleted,
		eaUrlFailed,
		eaUrlReturned
	};

	struct Aspect
	{
		EAspectAction action;
		DownloadQueue* downloadQueue;
		NzbInfo* nzbInfo;
		FileInfo* fileInfo;
	};

	enum EEditAction
	{
		eaFileMoveOffset = 1, // move files to m_iOffset relative to the current position in download-queue
		eaFileMoveTop, // move files to the top of download-queue
		eaFileMoveBottom, // move files to the bottom of download-queue
		eaFilePause, // pause files
		eaFileResume, // resume (unpause) files
		eaFileDelete, // delete files
		eaFilePauseAllPars, // pause only (all) pars (does not affect other files)
		eaFilePauseExtraPars, // pause (almost all) pars, except main par-file (does not affect other files)
		eaFileReorder, // set file order
		eaFileSplit, // split - create new group from selected files
		eaGroupMoveOffset, // move group to offset relative to the current position in download-queue
		eaGroupMoveTop, // move group to the top of download-queue
		eaGroupMoveBottom, // move group to the bottom of download-queue
		eaGroupMoveBefore, // move group to a certain position
		eaGroupMoveAfter, // move group to a certain position
		eaGroupPause, // pause group
		eaGroupResume, // resume (unpause) group
		eaGroupDelete, // delete group and put to history, delete already downloaded files
		eaGroupParkDelete, // delete group and put to history, keep already downloaded files
		eaGroupDupeDelete, // delete group, put to history and mark as duplicate, delete already downloaded files
		eaGroupFinalDelete, // delete group without adding to history, delete already downloaded files
		eaGroupPauseAllPars, // pause only (all) pars (does not affect other files) in group
		eaGroupPauseExtraPars, // pause only (almost all) pars in group, except main par-file (does not affect other files)
		eaGroupSetPriority, // set priority for groups
		eaGroupSetCategory, // set or change category for a group
		eaGroupApplyCategory, // set or change category for a group and reassign pp-params according to category settings
		eaGroupMerge, // merge groups
		eaGroupSetParameter, // set post-process parameter for group
		eaGroupSetName, // set group name (rename group)
		eaGroupSetDupeKey, // set duplicate key
		eaGroupSetDupeScore, // set duplicate score
		eaGroupSetDupeMode, // set duplicate mode
		eaGroupSort, // sort groups
		eaGroupSortFiles, // sort files for optimal download order
		eaPostDelete, // cancel post-processing
		eaHistoryDelete, // hide history-item
		eaHistoryFinalDelete, // delete history-item
		eaHistoryReturn, // move history-item back to download queue
		eaHistoryProcess, // move history-item back to download queue and start postprocessing
		eaHistoryRedownload, // move history-item back to download queue for full redownload
		eaHistoryRetryFailed, // move history-item back to download queue for redownload of failed articles
		eaHistorySetParameter, // set post-process parameter for history-item
		eaHistorySetDupeKey, // set duplicate key
		eaHistorySetDupeScore, // set duplicate score
		eaHistorySetDupeMode, // set duplicate mode
		eaHistorySetDupeBackup, // set duplicate backup flag
		eaHistoryMarkBad, // mark history-item as bad (and download other duplicate)
		eaHistoryMarkGood, // mark history-item as good (and push it into dup-history)
		eaHistoryMarkSuccess, // mark history-item as success (and do nothing more)
		eaHistorySetCategory, // set or change category for history-item
		eaHistorySetName // set history-item name (rename)
	};

	enum EMatchMode
	{
		mmId = 1,
		mmName,
		mmRegEx
	};

	static bool IsLoaded() { return g_Loaded; }
	static GuardedDownloadQueue Guard() { return GuardedDownloadQueue(g_DownloadQueue, &g_DownloadQueue->m_lockMutex); }
	NzbList* GetQueue() { return &m_queue; }
	HistoryList* GetHistory() { return &m_history; }
	virtual bool EditEntry(int ID, EEditAction action, const char* args) = 0;
	virtual bool EditList(IdList* idList, NameList* nameList, EMatchMode matchMode, EEditAction action, const char* args) = 0;
	virtual void HistoryChanged() = 0;
	virtual void Save() = 0;
	virtual void SaveChanged() = 0;
	void CalcRemainingSize(int64* remaining, int64* remainingForced);

protected:
	DownloadQueue() {}
	// a reload creates the queue anew: until it is loaded again, threads
	// waiting for IsLoaded() (the post-processor's queue sanitising, for
	// one) must not take the previous queue's state for it
	static void Init(DownloadQueue* globalInstance) { g_DownloadQueue = globalInstance; g_Loaded = false; }
	static void Final() { g_DownloadQueue = nullptr; }
	static void Loaded() { g_Loaded = true; }

private:
	NzbList m_queue;
	HistoryList m_history;
	Mutex m_lockMutex;

	static DownloadQueue* g_DownloadQueue;
	static std::atomic<bool> g_Loaded;
};


class ServerVolume;
using ServerVolumes = std::vector<ServerVolume>;
class StateDiskFile;

class DiskState
{
public:
	bool DownloadQueueExists();
	bool SaveDownloadQueue(DownloadQueue* downloadQueue, bool saveHistory);
	bool LoadDownloadQueue(DownloadQueue* downloadQueue, Servers* servers);
	/* the last load read a queue, history or progress file an earlier build of the
	 * branch wrote (formats 65/66): saving it again writes the upstream format */
	bool LegacyQueueRead() { return m_legacyQueueRead; }
	bool SaveDownloadProgress(DownloadQueue* downloadQueue);
	bool SaveFile(FileInfo* fileInfo, bool replace = false);
	bool LoadFile(FileInfo* fileInfo, bool fileSummary, bool articles);
	bool SaveAllFileInfos(DownloadQueue* downloadQueue);
	void DiscardQuickFileInfos();
	bool SaveFileState(FileInfo* fileInfo, bool completed, bool replace = false);
	bool LoadFileState(FileInfo* fileInfo, Servers* servers, bool completed);
	bool LoadArticles(FileInfo* fileInfo);
	void DiscardDownloadQueue();
	void DiscardFile(int fileId, bool deleteData, bool deletePartialState, bool deleteCompletedState);
	void DiscardFiles(NzbInfo* nzbInfo, bool deleteLog = true);
	bool SaveFeeds(Feeds* feeds, FeedHistory* feedHistory);
	bool LoadFeeds(Feeds* feeds, FeedHistory* feedHistory);
	bool SaveStats(Servers* servers, ServerVolumes* serverVolumes);
	bool LoadStats(Servers* servers, ServerVolumes* serverVolumes, bool* perfectMatch);
	void CleanupTempDir(DownloadQueue* downloadQueue);
	void WriteCacheFlag();
	void DeleteCacheFlag();
	void AppendNzbMessage(int nzbId, Message::EKind kind, const char* text);
	void LoadNzbMessages(int nzbId, MessageList* messages);

private:
	// the format version of the file state LoadFileState read last
	int m_lastFileStateVersion = 0;
	bool SaveFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, bool articles);
	bool LoadFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, int formatVersion, bool fileSummary, bool articles);
	bool SaveFileState(FileInfo* fileInfo, StateDiskFile& outfile, bool completed);
	bool LoadFileState(FileInfo* fileInfo, Servers* servers, StateDiskFile& infile, int formatVersion, bool completed);
	void SaveQueue(NzbList* queue, StateDiskFile& outfile);
	bool LoadQueue(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion);
	void SaveProgress(NzbList* queue, StateDiskFile& outfile, int changedCount);
	bool LoadProgress(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion);
	/* a queue, history or progress file that couldn't be read is renamed
	 * "<name>.unreadable-<time>": the next save would overwrite it */
	void SetAsideQueueFiles();
	void RaiseIdsPastQueueDir();
	void SaveIdFloor();
	void ConvertLegacyFileStates(Servers* servers);
	bool m_legacyQueueRead = false;
	void SaveNzbInfo(NzbInfo* nzbInfo, StateDiskFile& outfile);
	// this build's own data (duplicate repair) after upstream's in a queue file
	void SaveDupeExtras(NzbInfo* nzbInfo, StateDiskFile& outfile);
	bool LoadDupeExtras(NzbInfo* nzbInfo, StateDiskFile& infile, int formatVersion);
	void SaveDupeTrailer(const std::vector<NzbInfo*>& nzbs, StateDiskFile& outfile);
	bool LoadDupeTrailer(const std::function<NzbInfo*(int)>& find, StateDiskFile& infile);
	bool LoadNzbInfo(NzbInfo* nzbInfo, Servers* servers, StateDiskFile& infile, int formatVersion);
	void SaveDupInfo(DupInfo* dupInfo, StateDiskFile& outfile);
	bool LoadDupInfo(DupInfo* dupInfo, StateDiskFile& infile, int formatVersion);
	void SaveHistory(HistoryList* history, StateDiskFile& outfile);
	bool LoadHistory(HistoryList* history, Servers* servers, StateDiskFile& infile, int formatVersion);
	bool SaveFeedStatus(Feeds* feeds, StateDiskFile& outfile);
	bool LoadFeedStatus(Feeds* feeds, StateDiskFile& infile, int formatVersion);
	bool SaveFeedHistory(FeedHistory* feedHistory, StateDiskFile& outfile);
	bool LoadFeedHistory(FeedHistory* feedHistory, StateDiskFile& infile, int formatVersion);
	bool SaveServerInfo(Servers* servers, StateDiskFile& outfile);
	bool LoadServerInfo(Servers* servers, StateDiskFile& infile, int formatVersion, bool* perfectMatch);
	bool SaveVolumeStat(ServerVolumes* serverVolumes, StateDiskFile& outfile);
	bool LoadVolumeStat(Servers* servers, ServerVolumes* serverVolumes, StateDiskFile& infile, int formatVersion);
	void CalcFileStats(DownloadQueue* downloadQueue, int formatVersion);
	bool LoadAllFileInfos(DownloadQueue* downloadQueue);
	bool LoadAllFileStates(DownloadQueue* downloadQueue, Servers* servers);
	bool LoadServerStats(ServerStatList* serverStatList, Servers* servers, StateDiskFile& infile);
	void CleanupQueueDir(DownloadQueue* downloadQueue);
};

extern DiskState* g_DiskState;


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef SERVERPOOL_H

#include <algorithm>
#include <array>
#include <atomic>
#include <exception>
#include <functional>
#include <iomanip>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <vector>
#include "nzbget_rs.h"

#ifndef SERVERPOOL_H
#define SERVERPOOL_H

#ifndef NNTPCONNECTION_H
#define NNTPCONNECTION_H

class NntpConnection : public Connection
{
public:
	static constexpr int CONNECTION_LINEBUFFER_SIZE = 1024 * 10;

	NntpConnection(NewsServer* newsServer) :
		Connection(newsServer->GetHost(), newsServer->GetPort(), newsServer->GetTls()), m_newsServer(newsServer)
	{
		m_lineBuf.Reserve(CONNECTION_LINEBUFFER_SIZE);
		SetCipher(newsServer->GetCipher());
		SetIPVersion((Connection::EIPVersion)nzbget_rs_ntw_nntp_map_ip_version(
			newsServer->GetIpVersion(),
			(int)Connection::ipV4,
			(int)Connection::ipV6,
			(int)Connection::ipAuto));

#ifndef DISABLE_TLS
		SetCertVerifLevel(newsServer->GetCertVerificationLevel());
#endif
	}

	virtual bool Connect()
	{
		debug("Opening connection to %s", GetHost());

		if (m_status == csConnected)
		{
			return true;
		}
		m_authRejected = false;
		m_activeGroup = nullptr;

		if (!Connection::Connect())
		{
			return false;
		}

		// rust/src/nntp.rs: the greeting, and the login when one is set
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		int result = 0;
		nzbget_rs_nntp_handshake(&io, &authError, &authRejected, &result);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		int hsStatus = nzbget_rs_dqh_nntp_conn_classify_handshake_result(result);
		if (hsStatus == 1)
		{
			Disconnect();
			return false;
		}
		return hsStatus == 0;
	}

	virtual bool Disconnect()
	{
		if (m_status == csConnected)
		{
			Request("quit\r\n");
		}
		m_activeGroup = nullptr;
		return Connection::Disconnect();
	}

	NewsServer* GetNewsServer() { return m_newsServer; }

	const char* Request(const char* req)
	{
		if (!req)
		{
			return nullptr;
		}

		// rust/src/nntp.rs: the request, and a login when the server asks for one
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		char* answer = nullptr;
		nzbget_rs_nntp_request(&io, req, &authError, &authRejected, &answer);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		return answer;
	}

	const char* JoinGroup(const char* grp)
	{
		if (nzbget_rs_dqh_nntp_conn_is_already_in_group(*m_activeGroup, grp) != 0)
		{
			// already in group
			strcpy(m_lineBuf, "211 ");
			return m_lineBuf;
		}

		// rust/src/nntp.rs
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		char* answer = nullptr;
		int joined = 0;
		nzbget_rs_nntp_join_group(&io, grp, &authError, &authRejected, &answer, &joined);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		if (joined)
		{
			m_activeGroup = grp;
		}
		return answer;
	}

	bool GetAuthError() { return m_authError; }
	// the server refused the login for good (502), not for now (481: too many connections)
	bool GetAuthRejected() { return m_authRejected; }

private:
	NewsServer* m_newsServer;
	CString m_activeGroup;
	CharBuffer m_lineBuf;
	bool m_authError = false;
	bool m_authRejected = false;

#ifdef DEBUG
	static constexpr bool RsDebugMessages = true;
#else
	static constexpr bool RsDebugMessages = false;
#endif

	// the connection as rust/src/nntp.rs drives it (NzbgetRsNntpIo)
	struct RsExchange
	{
		NntpConnection* connection;
		std::exception_ptr error;

		template <typename F>
		static int Call(void* ctx, F f) noexcept
		{
			RsExchange* exchange = static_cast<RsExchange*>(ctx);
			try
			{
				f(exchange->connection);
				return 0;
			}
			catch (...)
			{
				exchange->error = std::current_exception();
				return -1;
			}
		}

		NzbgetRsNntpIo Io()
		{
			NewsServer* server = connection->m_newsServer;
			return {this, RsWriteLine, RsReadLine, RsReportError, RsDebugMessages ? RsDebug : nullptr, RsCancelled, server->GetUser(),
				server->GetPassword(), server->GetName(), server->GetHost(), connection->GetHost()};
		}

		void Rethrow()
		{
			if (error)
			{
				std::rethrow_exception(error);
			}
		}
	};

	static int RsWriteLine(void* ctx, const char* line)
	{
		return RsExchange::Call(ctx, [line](NntpConnection* c) { c->WriteLine(line); });
	}

	static int RsReadLine(void* ctx, char** line)
	{
		return RsExchange::Call(ctx, [line](NntpConnection* c) { *line = c->ReadLine(c->m_lineBuf, c->m_lineBuf.Size(), nullptr); });
	}

	static int RsReportError(void* ctx, const char* prefix, const char* arg)
	{
		return RsExchange::Call(ctx, [prefix, arg](NntpConnection* c) { c->ReportError(prefix, arg, false, 0); });
	}

	static int RsDebug(void* ctx, const char* msg)
	{
		return RsExchange::Call(ctx, [msg](NntpConnection*) { debug("%s", msg); });
	}

	static int RsCancelled(void* ctx, int* cancelled)
	{
		return RsExchange::Call(ctx, [cancelled](NntpConnection* c) { *cancelled = c->GetStatus() == csCancelled; });
	}
};

#endif

class ServerPool : public Debuggable
{
public:
	typedef std::vector<NewsServer*> RawServerList;

	void SetTimeout(int timeout) { m_timeout = timeout; }
	void SetRetryInterval(int retryInterval) { m_retryInterval = retryInterval; }
	void AddServer(std::unique_ptr<NewsServer> newsServer);
	void InitConnections();
	int GetMaxNormLevel() { return m_maxNormLevel; }
	Servers* GetServers() { return &m_servers; } // Only for read access (no lockings)
	NewsServer* GetServerById(int id);
	NntpConnection* GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers);
	void FreeConnection(NntpConnection* connection, bool used);
	void CloseUnusedConnections();
	void Changed();
	int GetGeneration() { return m_generation; }
	void BlockServer(NewsServer* newsServer);
	bool IsServerBlocked(NewsServer* newsServer);

protected:
	virtual void LogDebugInfo();

private:
	class PooledConnection : public NntpConnection
	{
	public:
		using NntpConnection::NntpConnection;
		bool GetInUse() { return m_inUse; }
		void SetInUse(bool inUse) { m_inUse = inUse; }
		time_t GetFreeTime() { return m_freeTime; }
		void SetFreeTimeNow();
	private:
		bool m_inUse = false;
		time_t m_freeTime = 0;
	};

	typedef std::vector<int> Levels;
	typedef std::vector<std::unique_ptr<PooledConnection>> Connections;

	Servers m_servers;
	RawServerList m_sortedServers;
	Connections m_connections;
	Levels m_levels;
	int m_maxNormLevel = 0;
	Mutex m_connectionsMutex;
	int m_timeout = 60;
	int m_retryInterval = 0;
	int m_generation = 0;

	void NormalizeLevels();
};

extern ServerPool* g_ServerPool;

#include <algorithm>
#include "nzbget_rs.h"

inline constexpr int CONNECTION_HOLD_SECODNS = 5;

inline void ServerPool::PooledConnection::SetFreeTimeNow()
{
	m_freeTime = Util::CurrentTime();
}

inline NewsServer* ServerPool::GetServerById(int id)
{
	auto serverIt = std::find_if(
		begin(m_servers),
		end(m_servers),
		[&](const std::unique_ptr<NewsServer>& server)
		{
			return server->GetId() == id;
		}
	);

	if (serverIt != end(m_servers))
	{
		return serverIt->get();
	}

	return nullptr;
}

inline void ServerPool::AddServer(std::unique_ptr<NewsServer> newsServer)
{
	debug("Adding server to ServerPool");

	m_servers.push_back(std::move(newsServer));
	m_sortedServers.push_back(m_servers.back().get());
}

inline void ServerPool::NormalizeLevels()
{
	if (m_servers.empty())
	{
		return;
	}

	size_t count = m_sortedServers.size();
	std::vector<NzbgetRsServerNormInput> inputs(count);
	std::vector<size_t> sortedIndices(count);
	std::vector<int> normLevels(count);
	std::vector<NewsServer*> origServers = m_sortedServers;
	for (size_t i = 0; i < count; i++)
	{
		inputs[i].level = origServers[i]->GetLevel();
		inputs[i].active = origServers[i]->GetActive() ? 1 : 0;
		inputs[i].max_connections = origServers[i]->GetMaxConnections();
	}
	m_maxNormLevel = nzbget_rs_server_pool_normalize_levels(
		inputs.data(), count, sortedIndices.data(), normLevels.data());
	for (size_t i = 0; i < count; i++)
	{
		origServers[i]->SetNormLevel(normLevels[i]);
		m_sortedServers[i] = origServers[sortedIndices[i]];
	}
}

inline void ServerPool::InitConnections()
{
	debug("Initializing connections in ServerPool");

	Guard guard(m_connectionsMutex);

	NormalizeLevels();
	m_levels.clear();

	for (NewsServer* newsServer : m_sortedServers)
	{
		newsServer->SetBlockTime(0);
		int normLevel = newsServer->GetNormLevel();
		if (newsServer->GetNormLevel() > -1)
		{
			if ((int)m_levels.size() <= normLevel)
			{
				m_levels.push_back(0);
			}

			if (newsServer->GetActive())
			{
				int connections = 0;

				for (PooledConnection* connection : &m_connections)
				{
					if (connection->GetNewsServer() == newsServer)
					{
						connections++;
					}
				}

				for (int i = connections; i < newsServer->GetMaxConnections(); i++)
				{
					std::unique_ptr<PooledConnection> connection = std::make_unique<PooledConnection>(newsServer);
					connection->SetTimeout(m_timeout);
					m_connections.push_back(std::move(connection));
					connections++;
				}

				m_levels[normLevel] += connections;
			}
		}
	}

	m_generation++;
}

inline NntpConnection* ServerPool::GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers)
{
	Guard guard(m_connectionsMutex);

	if (level < 0 || m_sortedServers.empty() || m_connections.empty())
	{
		return nullptr;
	}

	size_t srvCount = m_sortedServers.size();
	std::vector<int32_t> srvIds(srvCount);
	std::vector<int32_t> srvGroups(srvCount);
	std::vector<int32_t> srvNormLevels(srvCount);
	std::vector<uint8_t> srvActive(srvCount);
	std::vector<uint8_t> srvOptional(srvCount);
	std::vector<uint8_t> srvBlocked(srvCount);
	for (size_t i = 0; i < srvCount; ++i)
	{
		NewsServer* srv = m_sortedServers[i];
		srvIds[i] = srv->GetId();
		srvGroups[i] = srv->GetGroup();
		srvNormLevels[i] = srv->GetNormLevel();
		srvActive[i] = srv->GetActive() ? 1 : 0;
		srvOptional[i] = srv->GetOptional() ? 1 : 0;
		srvBlocked[i] = IsServerBlocked(srv) ? 1 : 0;
	}

	size_t connCount = m_connections.size();
	std::vector<size_t> connSrvIndices(connCount);
	std::vector<uint8_t> connInUse(connCount);
	std::vector<uint8_t> connConnected(connCount);
	for (size_t i = 0; i < connCount; ++i)
	{
		PooledConnection* conn = m_connections[i].get();
		NewsServer* srv = conn->GetNewsServer();
		size_t sIdx = srvCount;
		for (size_t j = 0; j < srvCount; ++j)
		{
			if (m_sortedServers[j] == srv)
			{
				sIdx = j;
				break;
			}
		}
		connSrvIndices[i] = sIdx;
		connInUse[i] = conn->GetInUse() ? 1 : 0;
		connConnected[i] = conn->GetStatus() == Connection::csConnected ? 1 : 0;
	}

	std::vector<int32_t> ignIds;
	std::vector<int32_t> ignGroups;
	std::vector<int32_t> ignNormLevels;
	if (ignoreServers && !wantServer)
	{
		ignIds.reserve(ignoreServers->size());
		ignGroups.reserve(ignoreServers->size());
		ignNormLevels.reserve(ignoreServers->size());
		for (NewsServer* ign : *ignoreServers)
		{
			ignIds.push_back(ign->GetId());
			ignGroups.push_back(ign->GetGroup());
			ignNormLevels.push_back(ign->GetNormLevel());
		}
	}

	std::vector<size_t> unblockIndices(srvCount);
	size_t unblockCount = 0;
	size_t chosenConnIdx = 0;
	int32_t chosenLevel = 0;
	int found = nzbget_rs_server_pool_select_connection(
		level,
		m_levels.empty() ? nullptr : m_levels.data(),
		m_levels.size(),
		srvIds.data(),
		srvGroups.data(),
		srvNormLevels.data(),
		srvActive.data(),
		srvOptional.data(),
		srvBlocked.data(),
		srvCount,
		connSrvIndices.data(),
		connInUse.data(),
		connConnected.data(),
		connCount,
		wantServer ? 1 : 0,
		wantServer ? wantServer->GetId() : 0,
		wantServer ? wantServer->GetGroup() : 0,
		ignIds.empty() ? nullptr : ignIds.data(),
		ignGroups.empty() ? nullptr : ignGroups.data(),
		ignNormLevels.empty() ? nullptr : ignNormLevels.data(),
		ignIds.size(),
		rand(),
		unblockIndices.data(),
		&unblockCount,
		&chosenConnIdx,
		&chosenLevel);

	for (size_t i = 0; i < unblockCount; ++i)
	{
		m_sortedServers[unblockIndices[i]]->SetBlockTime(0);
	}

	if (found != 0)
	{
		PooledConnection* connection = m_connections[chosenConnIdx].get();
		connection->SetInUse(true);
		m_levels[chosenLevel]--;
		return connection;
	}

	return nullptr;
}

inline void ServerPool::FreeConnection(NntpConnection* connection, bool used)
{
	if (used)
	{
		debug("Freeing used connection");
	}

	Guard guard(m_connectionsMutex);

	((PooledConnection*)connection)->SetInUse(false);
	if (used)
	{
		((PooledConnection*)connection)->SetFreeTimeNow();
	}

	if (connection->GetNewsServer()->GetNormLevel() > -1 && connection->GetNewsServer()->GetActive())
	{
		m_levels[connection->GetNewsServer()->GetNormLevel()]++;
	}
}

inline void ServerPool::BlockServer(NewsServer* newsServer)
{
	int newBlockInt = 0;
	int shouldWarnInt = 0;
	{
		Guard guard(m_connectionsMutex);
		time_t curTime = Util::CurrentTime();
		nzbget_rs_nntp_server_pool_eval_block_server(
			(int64_t)newsServer->GetBlockTime(),
			(int64_t)curTime,
			m_retryInterval,
			&newBlockInt,
			&shouldWarnInt);
		newsServer->SetBlockTime(curTime);
	}

	if (shouldWarnInt != 0)
	{
		warn("Blocking %s (%s) for %i sec", newsServer->GetName(), newsServer->GetHost(), m_retryInterval);
	}
}

inline bool ServerPool::IsServerBlocked(NewsServer* newsServer)
{
	if (!newsServer->GetBlockTime())
	{
		return false;
	}

	time_t curTime = Util::CurrentTime();
	return nzbget_rs_server_pool_is_blocked(newsServer->GetBlockTime(), curTime, m_retryInterval) != 0;
}

inline void ServerPool::CloseUnusedConnections()
{
	Guard guard(m_connectionsMutex);

	time_t curtime = Util::CurrentTime();

	m_connections.erase(std::remove_if(m_connections.begin(), m_connections.end(),
		[](std::unique_ptr<PooledConnection>& connection)
		{
			if (!connection->GetInUse() &&
				(connection->GetNewsServer()->GetNormLevel() == -1 || !connection->GetNewsServer()->GetActive()))
			{
				debug("Closing (and deleting) unused connection to server%i", connection->GetNewsServer()->GetId());
				if (connection->GetStatus() == Connection::csConnected)
				{
					connection->Disconnect();
				}
				return true;
			}
			return false;
		}),
		m_connections.end());

	for (int level = 0; level <= m_maxNormLevel; level++)
	{
		std::vector<uint8_t> inUse;
		std::vector<uint8_t> connected;
		std::vector<int64_t> freeTimes;
		for (PooledConnection* connection : &m_connections)
		{
			if (connection->GetNewsServer()->GetNormLevel() == level)
			{
				inUse.push_back(connection->GetInUse() ? 1 : 0);
				connected.push_back(connection->GetStatus() == Connection::csConnected ? 1 : 0);
				freeTimes.push_back((int64_t)connection->GetFreeTime());
			}
		}
		if (nzbget_rs_server_pool_level_should_close(
			inUse.empty() ? nullptr : inUse.data(),
			connected.empty() ? nullptr : connected.data(),
			freeTimes.empty() ? nullptr : freeTimes.data(),
			inUse.size(), (int64_t)curtime, CONNECTION_HOLD_SECODNS) != 0)
		{
			for (PooledConnection* connection : &m_connections)
			{
				if (nzbget_rs_dqh_server_pool_should_disconnect_idle_level_conn(
					connection->GetNewsServer()->GetNormLevel(), level,
					(int)connection->GetStatus(), (int)Connection::csConnected) != 0)
				{
					debug("Closing (and keeping) unused connection to server%i", connection->GetNewsServer()->GetId());
					connection->Disconnect();
				}
			}
		}
	}
}

inline void ServerPool::Changed()
{
	debug("Server config has been changed");

	InitConnections();
	CloseUnusedConnections();
}

inline void ServerPool::LogDebugInfo()
{
	info("   ---------- ServerPool");

	info("    Max-Level: %i", m_maxNormLevel);

	Guard guard(m_connectionsMutex);

	time_t curTime = Util::CurrentTime();

	info("    Servers: %i", (int)m_servers.size());
	for (NewsServer* newsServer : &m_servers)
	{
		int blockSec = nzbget_rs_ntw_server_pool_calc_remaining_block_sec(
			(int64_t)newsServer->GetBlockTime(), m_retryInterval, (int64_t)curTime);
		info("      %i) %s (%s): Level=%i, NormLevel=%i, BlockSec=%i", newsServer->GetId(), newsServer->GetName(),
			newsServer->GetHost(), newsServer->GetLevel(), newsServer->GetNormLevel(), blockSec);
	}

	info("    Levels: %i", (int)m_levels.size());
	int index = 0;
	for (int size : m_levels)
	{
		info("      %i: Free connections=%i", index, size);
		index++;
	}

	info("    Connections: %i", (int)m_connections.size());
	for (PooledConnection* connection : &m_connections)
	{
		info("      %i) %s (%s): Level=%i, NormLevel=%i, InUse:%i", connection->GetNewsServer()->GetId(),
			connection->GetNewsServer()->GetName(), connection->GetNewsServer()->GetHost(),
			connection->GetNewsServer()->GetLevel(), connection->GetNewsServer()->GetNormLevel(),
			(int)connection->GetInUse());
	}
}

#ifndef NZBGET_TLS_CIPHER_BACKEND_H
#define NZBGET_TLS_CIPHER_BACKEND_H

#if !defined(DISABLE_TLS) && (defined(__APPLE__) || defined(__linux__))
#include <openssl/rand.h>
#include <array>
#include <limits>
#include <memory>
#include <vector>

namespace TlsCipherBackend
{
	namespace Detail
	{
		constexpr int RecordSize = 16384;
		struct Context { std::string snapshot; };
		struct Crypto
		{
			const EVP_CIPHER* cipher = nullptr;
			OpenSSL::EVPCipherCtxPtr encrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			OpenSSL::EVPCipherCtxPtr decrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			std::array<unsigned char, 64> key{};
			std::array<unsigned char, 4> prefix{};
			std::array<unsigned char, RecordSize> plaintext{};
			std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH> output{};
			std::vector<std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH>> ciphertext;
			std::vector<std::array<unsigned char, 16>> tags;
			std::vector<std::array<unsigned char, 12>> nonces;
			uint64_t first = 0;
			int tagSize = 16;
			bool ccm = false;
			bool encryptKeyed = false, decryptKeyed = false;
			~Crypto() { OPENSSL_cleanse(key.data(), key.size()); }
			bool Init(std::string name)
			{
				if (name.ends_with("@8")) { tagSize = 8; name.resize(name.size() - 2); }
				cipher = EVP_get_cipherbyname(name.c_str());
				if (!cipher || !encrypt || !decrypt) return false;
				int mode = EVP_CIPHER_mode(cipher), keySize = EVP_CIPHER_key_length(cipher);
				ccm = mode == EVP_CIPH_CCM_MODE;
				if (mode != EVP_CIPH_GCM_MODE && !ccm && EVP_CIPHER_nid(cipher) != NID_chacha20_poly1305) return false;
				if (keySize <= 0 || keySize > static_cast<int>(key.size()) || RAND_bytes(key.data(), keySize) != 1 || RAND_bytes(prefix.data(), prefix.size()) != 1) return false;
				nzbget_rs_dqh_tls_cipher_fill_test_plaintext(plaintext.data(), RecordSize);
				return EVP_EncryptInit_ex(encrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_DecryptInit_ex(decrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1;
			}
			bool Prepare(uint64_t sequence, uint32_t count)
			{
				if (!nzbget_rs_dqh_tls_cipher_is_prepare_batch_valid(sequence, count)) return false;
				first = sequence; ciphertext.resize(count); tags.resize(count); nonces.resize(count);
				for (uint32_t i = 0; i < count; ++i)
				{
					auto& nonce = nonces[i];
					nzbget_rs_dqh_tls_cipher_write_record_nonce(prefix.data(), sequence + i, nonce.data());
					int size = 0, tail = 0;
					unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
					if (ccm && EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, nullptr) != 1) return false;
					if (EVP_EncryptInit_ex(encrypt.get(), nullptr, nullptr, encryptKeyed ? nullptr : key.data(), nonce.data()) != 1) return false;
					encryptKeyed = true;
					if (ccm && EVP_EncryptUpdate(encrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return false;
					if (EVP_EncryptUpdate(encrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
						EVP_EncryptUpdate(encrypt.get(), ciphertext[i].data(), &size, plaintext.data(), RecordSize) != 1 || size != RecordSize ||
						EVP_EncryptFinal_ex(encrypt.get(), ciphertext[i].data() + size, &tail) != 1 || tail != 0 ||
						EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_GET_TAG, tagSize, tags[i].data()) != 1) return false;
				}
				if (Record(sequence) != RecordSize) return false;
				tags[0][0] ^= 1;
				bool rejected = Record(sequence) == -1;
				tags[0][0] ^= 1;
				ERR_clear_error();
				return rejected && Record(sequence) == RecordSize;
			}
			int Record(uint64_t sequence)
			{
				if (!nzbget_rs_dqh_tls_cipher_is_record_seq_in_range(sequence, first, ciphertext.size())) return -1;
				size_t i = static_cast<size_t>(sequence - first);
				unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
				int size = 0, tail = 0;
				if (ccm && EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1) return -1;
				if (EVP_DecryptInit_ex(decrypt.get(), nullptr, nullptr, decryptKeyed ? nullptr : key.data(), nonces[i].data()) != 1) return -1;
				decryptKeyed = true;
				if (ccm && EVP_DecryptUpdate(decrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return -1;
				if (EVP_DecryptUpdate(decrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
					EVP_DecryptUpdate(decrypt.get(), output.data(), &size, ciphertext[i].data(), RecordSize) != 1 || size != RecordSize) return -1;
				if (!ccm && (EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1 ||
					EVP_DecryptFinal_ex(decrypt.get(), output.data() + size, &tail) != 1 || tail != 0)) return -1;
				return nzbget_rs_dqh_mem_slices_equal(output.data(), plaintext.data(), RecordSize) != 0 ? RecordSize : -1;
			}
		};

		inline int Snapshot(void* context, const unsigned char** data, size_t* length) noexcept
		{
			try
			{
				auto& value = static_cast<Context*>(context)->snapshot;
				if (!g_ScriptConfig->ReadCipherSnapshot(value)) return 0;
				*data = reinterpret_cast<const unsigned char*>(value.data()); *length = value.size(); return 1;
			}
			catch (...) { return 0; }
		}
		inline int Suites(void*, NzbgetTlsEmitSuite emit, void* context) noexcept
		{
			try
			{
				OpenSSL::SSLCtxPtr tls(SSL_CTX_new(TLS_method()), &SSL_CTX_free);
				if (!tls) return 0;
				int minimum = SSL_CTX_get_min_proto_version(tls.get()), maximum = SSL_CTX_get_max_proto_version(tls.get());
				const auto* suites = SSL_CTX_get_ciphers(tls.get());
				for (int i = 0; i < sk_SSL_CIPHER_num(suites); ++i)
				{
					const auto* suite = sk_SSL_CIPHER_value(suites, i);
					const char* suiteName = SSL_CIPHER_get_name(suite);
					bool tls13 = nzbget_rs_dqh_tls_is_tls13_version_str(SSL_CIPHER_get_version(suite)) != 0;
					int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
					if (nzbget_rs_dqh_tls_is_proto_version_out_of_bounds(version, minimum, maximum) != 0) continue;
					int auth = SSL_CIPHER_get_auth_nid(suite), kx = SSL_CIPHER_get_kx_nid(suite);
					unsigned flags = nzbget_rs_dqh_tls_compute_suite_flags(
						SSL_CIPHER_is_aead(suite) ? 1 : 0,
						tls13 ? 1 : 0,
						(auth == NID_auth_rsa || auth == NID_auth_ecdsa) ? 1 : 0,
						(kx == NID_kx_ecdhe || kx == NID_kx_dhe) ? 1 : 0);
					const EVP_CIPHER* bulk = EVP_get_cipherbynid(SSL_CIPHER_get_cipher_nid(suite));
					NzbgetRsBuf bulkBuf = nzbget_rs_dqh_tls_format_bulk_cipher_name(
						bulk ? EVP_CIPHER_name(bulk) : nullptr, suiteName);
					unsigned bits = nzbget_rs_dqh_tls_resolve_cipher_bits(
						suiteName, (unsigned)SSL_CIPHER_get_bits(suite, nullptr));
					int emitted = emit(context, suiteName, bulkBuf.data ? bulkBuf.data : "", tls13 ? 13 : 12, bits, flags);
					nzbget_rs_free(bulkBuf);
					if (!emitted) return 0;
				}
				return 1;
			}
			catch (...) { return 0; }
		}
		inline void* Prepare(void*, const char* algorithm) noexcept
		{
			try { auto crypto = std::make_unique<Crypto>(); return crypto->Init(algorithm) ? crypto.release() : nullptr; }
			catch (...) { return nullptr; }
		}
		inline int PrepareBatch(void* context, uint64_t first, uint32_t count) noexcept
		{
			try { return static_cast<Crypto*>(context)->Prepare(first, count) ? 1 : 0; } catch (...) { return 0; }
		}
		inline int Record(void* context, uint64_t sequence) noexcept
		{
			try { return static_cast<Crypto*>(context)->Record(sequence); } catch (...) { return -1; }
		}
		inline void FreeCrypto(void* context) noexcept { delete static_cast<Crypto*>(context); ERR_clear_error(); }

		class ProbeConnection final : public NntpConnection
		{
		public:
			explicit ProbeConnection(NewsServer* server) : NntpConnection(server) {}
		protected:
			void PrintError(const char*) override {}
		};
		inline int Probe(void*, const NzbgetTlsServer* server, const char* cipher, int protocol, const char* numeric,
			uint64_t milliseconds, NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				int precheck = nzbget_rs_dqh_tls_cipher_precheck_probe_args(
					cancel(cancelContext) ? 1 : 0,
					milliseconds,
					server->verification ? 1 : 0,
					TlsSocket::HasCertificateStore() ? 1 : 0);
				if (precheck != 0) return precheck;
				auto deadline = std::make_shared<SocketDeadline>();
				deadline->end = std::chrono::steady_clock::now() + std::chrono::milliseconds(milliseconds);
				deadline->cancelled = [=] { return cancel(cancelContext) != 0; };
				NewsServer value(server->id, true, "TLS cipher probe", server->host, server->port, server->ip_version,
					server->username, server->password, false, true, cipher, 1, 0, 0, 0, false, server->verification);
				ProbeConnection connection(&value);
				std::vector<std::string> addresses;
				std::string remaining(numeric);
				for (size_t pos = 0; pos < remaining.size();)
				{
					size_t end = remaining.find('\n', pos);
					addresses.push_back(remaining.substr(pos, end == std::string::npos ? end : end - pos));
					if (end == std::string::npos) break; pos = end + 1;
				}
				if (!nzbget_rs_dqh_tls_cipher_is_probe_addr_list_valid(addresses.size())) return -2;
				connection.SetResolvedAddresses(addresses); connection.SetDeadline(deadline);
				connection.SetSuppressErrors(true); connection.SetTimeout(10);
				bool connected = connection.Connect();
				int result = nzbget_rs_dqh_tls_resolve_probe_result(
					connected ? 1 : 0,
					connection.GetNegotiatedCipher(),
					cipher,
					connection.GetNegotiatedVersion(),
					protocol,
					TLS1_3_VERSION,
					TLS1_2_VERSION,
					connection.GetAuthError() ? 1 : 0,
					connection.GetTlsProbeFailure());
				result = nzbget_rs_dqh_tls_cipher_classify_probe_deadline_override(
					result,
					cancel(cancelContext) ? 1 : 0,
					std::chrono::steady_clock::now() >= deadline->end ? 1 : 0);
				connection.SetBroken(); connection.Disconnect();
				return result;
			}
			catch (...) { ERR_clear_error(); return -1; }
		}
		inline int Commit(void*, const unsigned char* original, size_t originalLength, const unsigned char* replacement, size_t replacementLength,
			NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				auto result = g_ScriptConfig->CommitCipherPatch(
					std::string_view(reinterpret_cast<const char*>(original), originalLength),
					std::string_view(reinterpret_cast<const char*>(replacement), replacementLength), [=] { return cancel(cancelContext) != 0; });
				switch (result)
				{
					case ScriptConfig::CipherCommit::Saved: return 1;
					case ScriptConfig::CipherCommit::Unchanged: return 0;
					case ScriptConfig::CipherCommit::Conflict: return -2;
					case ScriptConfig::CipherCommit::Cancelled: return -3;
					default: return -1;
				}
			}
			catch (...) { return -1; }
		}
		inline void Destroy(void* context) noexcept { delete static_cast<Context*>(context); }
		inline std::string Response(NzbgetRsBuf value)
		{
			struct Release { NzbgetRsBuf value; ~Release() { nzbget_rs_free(value); } } release{value};
			return std::string(value.data, value.len);
		}
	}

	inline bool Available() { return true; }
	inline NzbgetTlsBackend Create()
	{
		return {new Detail::Context(), Detail::Snapshot, Detail::Suites, Detail::Prepare, Detail::PrepareBatch, Detail::Record, Detail::FreeCrypto, Detail::Probe, Detail::Commit, Detail::Destroy};
	}
	inline std::string Start()
	{
		NzbgetTlsBackend io = Create();
		return Detail::Response(nzbget_rs_tls_cipher_start(&io));
	}
	inline std::string Status(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_status(id)); }
	inline std::string Cancel(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_cancel(id)); }
	inline void Shutdown() { nzbget_rs_tls_cipher_shutdown(); }
}
#else
namespace TlsCipherBackend
{
	inline bool Available() { return false; }
	inline std::string Status(const char*) { return R"({"Available":false,"State":"unavailable","Message":"TLS cipher optimization requires a TLS-enabled Rust backend and a supported atomic configuration adapter."})"; }
	inline std::string Start() { return Status(nullptr); }
	inline std::string Cancel(const char*) { return Status(nullptr); }
	inline void Shutdown() {}
	inline NzbgetTlsBackend Create() { return {}; }
}
#endif
#endif

#ifndef STATMETER_H
#define STATMETER_H

class ServerVolume
{
public:
	struct Articles
	{
		uint32 failed;
		uint32 success;
	};
	struct Stats
	{
		uint32 bytes;
		Articles articles;
	};

	using VolumeArray = std::vector<int64>;
	using ArticlesArray = std::vector<Articles>;

	VolumeArray* BytesPerSeconds() { return &m_bytesPerSeconds; }
	VolumeArray* BytesPerMinutes() { return &m_bytesPerMinutes; }
	VolumeArray* BytesPerHours() { return &m_bytesPerHours; }
	VolumeArray* BytesPerDays() { return &m_bytesPerDays; }

	const ArticlesArray& GetArticlesPerDays() const { return m_articlesPerDays; }
	void SetArticlesPerDays(ArticlesArray articles) { m_articlesPerDays = std::move(articles); }
	void SetFirstDay(int firstDay) { m_firstDay = firstDay; }
	int GetFirstDay() { return m_firstDay; }
	void SetTotalBytes(int64 totalBytes) { m_totalBytes = totalBytes; }
	int64 GetTotalBytes() { return m_totalBytes; }
	void SetCustomBytes(int64 customBytes) { m_customBytes = customBytes; }
	int64 GetCustomBytes() { return m_customBytes; }
	int GetSecSlot() { return m_secSlot; }
	int GetMinSlot() { return m_minSlot; }
	int GetHourSlot() { return m_hourSlot; }
	int GetDaySlot() { return m_daySlot; }
	time_t GetDataTime() { return m_dataTime; }
	void SetDataTime(time_t dataTime) { m_dataTime = dataTime; }
	time_t GetCustomTime() { return m_customTime; }
	void SetCustomTime(time_t customTime) { m_customTime = customTime; }
	time_t GetCountersResetTime() { return m_countersResetTime; }
	void SetCountersResetTime(time_t time) { m_countersResetTime = time; }

	void AddStats(Stats stats);
	void CalcSlots(time_t locCurTime);
	void Reset();
	void ResetCustom();
	void LogDebugInfo();

private:
	void ResetVolume(VolumeArray& volume);
	void ResetArticles();

	VolumeArray m_bytesPerSeconds = VolumeArray(60);
	VolumeArray m_bytesPerMinutes = VolumeArray(60);
	VolumeArray m_bytesPerHours = VolumeArray(24);
	VolumeArray m_bytesPerDays;
	ArticlesArray m_articlesPerDays;

	int m_firstDay = 0;
	int64 m_totalBytes = 0;
	int64 m_customBytes = 0;

	/** Date/time when the data was last updated (time is in C/Unix format) */
	time_t m_dataTime = 0;

	/** Date/time of the last reset of custom counter (time is in C/Unix format) */
	time_t m_customTime = Util::CurrentTime();

	/** Date/time of the last reset of all counters (time is in C/Unix format) */
	time_t m_countersResetTime = Util::CurrentTime();

	int m_secSlot = 0;
	int m_minSlot = 0;
	int m_hourSlot = 0;
	int m_daySlot = 0;
};

using ServerVolumes = std::vector<ServerVolume>;
using GuardedServerVolumes = GuardedPtr<ServerVolumes>;

class StatMeter : public Debuggable
{
public:
	StatMeter();
	void Init();
	int64 CalcCurrentDownloadSpeed();
	int64 CalcMomentaryDownloadSpeed();
	void AddSpeedReading(int bytes);
	void AddServerStats(ServerVolume::Stats stats, int serverId);
	void CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy);
	void CalcQuotaUsage(int64& monthBytes, int64& dayBytes);
	void IntervalCheck();
	void EnterLeaveStandBy(bool enter);
	GuardedServerVolumes GuardServerVolumes();
	// the next interval check saves the volumes (changed through the guard)
	void MarkChanged() { m_statChanged = true; }
	void Save();
	bool Load(bool* perfectServerMatch);

protected:
	virtual void LogDebugInfo();

private:
	// speed meter
	static const int SPEEDMETER_SLOTS = 30;
	static const int SPEEDMETER_SLOTSIZE = 1; //Split elapsed time into this number of secs.
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedBytes;
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedTime;
	std::atomic<time_t> m_speedCorrection{0};
	std::atomic<time_t> m_curSecTime{0};
	std::atomic<int64> m_speedTotalBytes{0};
	std::atomic<int> m_speedBytesIndex{0};
	std::atomic<int> m_curSecBytes{0};
	std::atomic<int> m_speedStartTime{0};
	std::mutex m_speedTotalBytesMtx;

	// time
	std::atomic<int64> m_allBytes{0};
	std::atomic<time_t> m_startServer{0};
	std::atomic<time_t> m_pausedFrom{0};
	std::atomic<time_t> m_startDownload{0};
	std::atomic<bool> m_standBy{true};
	time_t m_lastCheck = 0;
	time_t m_lastTimeOffset = 0;

	// data volume
	std::atomic<bool> m_statChanged{false};
	ServerVolumes m_serverVolumes;
	Mutex m_volumeMutex;

	void ResetSpeedStat();
	void AdjustTimeOffset();
	void CheckQuota();
	int CalcMonthSlots(const ServerVolume& volume);
};

extern StatMeter* g_StatMeter;

inline void ServerVolume::CalcSlots(time_t locCurTime)
{
	NzbgetRsVolumeSlots slots;
	nzbget_rs_volume_calc_slots(locCurTime, &m_firstDay, &slots);
	m_secSlot = slots.sec;
	m_minSlot = slots.min;
	m_hourSlot = slots.hour;
	m_daySlot = slots.day;
	if (slots.inRange)
	{
		size_t newBytesDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_bytesPerDays.size());
		if (newBytesDaysLen > 0)
		{
			m_bytesPerDays.resize(newBytesDaysLen);
		}

		size_t newArtDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_articlesPerDays.size());
		if (newArtDaysLen > 0)
		{
			m_articlesPerDays.resize(newArtDaysLen);
		}
	}
}

inline void ServerVolume::AddStats(Stats stats)
{
	time_t curTime = Util::CurrentTime();
	time_t locCurTime = curTime + g_WorkState->GetLocalTimeOffset();
	time_t locDataTime = m_dataTime + g_WorkState->GetLocalTimeOffset();

	int lastMinSlot = m_minSlot;
	int lastHourSlot = m_hourSlot;

	CalcSlots(locCurTime);

	int64 bytes = static_cast<int64>(stats.bytes);
	NzbgetRsVolumeSlots slots{m_secSlot, m_minSlot, m_hourSlot, m_daySlot, 0};
	nzbget_rs_volume_add(m_bytesPerSeconds.data(), m_bytesPerSeconds.size(), m_bytesPerMinutes.data(),
		m_bytesPerMinutes.size(), m_bytesPerHours.data(), m_bytesPerHours.size(), &slots, lastMinSlot,
		lastHourSlot, locCurTime, locDataTime, bytes);
	size_t daySlotIdx = m_daySlot >= 0 ? static_cast<size_t>(m_daySlot) : 0;
	int64 curDayBytes = m_daySlot >= 0 ? m_bytesPerDays[daySlotIdx] : 0;
	int curDayFailed = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].failed : 0;
	int curDaySuccess = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].success : 0;
	int updateDay = 0;
	int64 newDayBytes = 0, newTotalBytes = 0, newCustomBytes = 0;
	int newDayFailed = 0, newDaySuccess = 0;
	nzbget_rs_statmeter_accum_day_and_total_volume(
		m_daySlot, curDayBytes, curDayFailed, curDaySuccess,
		m_totalBytes, m_customBytes, bytes, stats.articles.failed, stats.articles.success,
		&updateDay, &newDayBytes, &newDayFailed, &newDaySuccess, &newTotalBytes, &newCustomBytes);
	if (updateDay != 0)
	{
		m_bytesPerDays[daySlotIdx] = newDayBytes;
		m_articlesPerDays[daySlotIdx].failed = newDayFailed;
		m_articlesPerDays[daySlotIdx].success = newDaySuccess;
	}
	m_totalBytes = newTotalBytes;
	m_customBytes = newCustomBytes;

	m_dataTime = curTime;
}

inline void ServerVolume::Reset()
{
	m_totalBytes = 0;
	m_dataTime = 0;
	m_firstDay = 0;
	m_countersResetTime = Util::CurrentTime();

	ResetVolume(m_bytesPerSeconds);
	ResetVolume(m_bytesPerMinutes);
	ResetVolume(m_bytesPerHours);
	ResetVolume(m_bytesPerDays);

	ResetArticles();
	ResetCustom();
}

inline void ServerVolume::ResetCustom()
{
	m_customBytes = 0;
	m_customTime = Util::CurrentTime();
}

inline void ServerVolume::ResetVolume(VolumeArray& volume)
{
	for (int64& size : volume)
	{
		size = 0;
	}
}

inline void ServerVolume::ResetArticles()
{
	for (Articles& articles : m_articlesPerDays)
	{
		articles.failed = 0;
		articles.success = 0;
	}
}

inline void ServerVolume::LogDebugInfo()
{
	info("   ---------- ServerVolume");

	NzbgetRsBuf secsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerSeconds.data(), m_bytesPerSeconds.size());
	info("Secs: %s", secsBuf.data ? secsBuf.data : "");
	nzbget_rs_free(secsBuf);

	NzbgetRsBuf minsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerMinutes.data(), m_bytesPerMinutes.size());
	info("Mins: %s", minsBuf.data ? minsBuf.data : "");
	nzbget_rs_free(minsBuf);

	NzbgetRsBuf hoursBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerHours.data(), m_bytesPerHours.size());
	info("Hours: %s", hoursBuf.data ? hoursBuf.data : "");
	nzbget_rs_free(hoursBuf);

	NzbgetRsBuf daysBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		m_firstDay, m_bytesPerDays.data(), m_bytesPerDays.size());
	info("Days: %s", daysBuf.data ? daysBuf.data : "");
	nzbget_rs_free(daysBuf);
}

inline StatMeter::StatMeter()
{
	debug("Creating StatMeter");

	ResetSpeedStat();
}

inline void StatMeter::Init()
{
	m_startServer = Util::CurrentTime();
	m_lastCheck = m_startServer.load();
	AdjustTimeOffset();

	m_serverVolumes.resize(1 + g_ServerPool->GetServers()->size());
}

inline void StatMeter::AdjustTimeOffset()
{
	time_t utcTime = Util::CurrentTime();
	tm tmSplittedTime;
	gmtime_r(&utcTime, &tmSplittedTime);
	tmSplittedTime.tm_isdst = -1;
	time_t locTime = mktime(&tmSplittedTime);
	int localDelta = 0;
	int totalOffset = nzbget_rs_statmeter_calc_local_time_offset(
		(int64_t)utcTime, (int64_t)locTime, g_Options->GetTimeCorrection(), &localDelta);
	g_WorkState->SetLocalTimeOffset(totalOffset);
	m_lastTimeOffset = utcTime;

	debug("UTC delta: %i (%i+%i)", g_WorkState->GetLocalTimeOffset(), localDelta, g_Options->GetTimeCorrection());
}

inline void StatMeter::IntervalCheck()
{
	time_t m_curTime = Util::CurrentTime();
	int64 clockDelta = 0;
	int adjustDownload = 0;
	int adjustTz = 0;
	if (nzbget_rs_sus_eval_interval_check(
		static_cast<int64>(m_curTime),
		static_cast<int64>(m_lastCheck),
		static_cast<int64>(m_lastTimeOffset),
		static_cast<int64>(m_startDownload.load()),
		m_standBy ? 1 : 0,
		&clockDelta,
		&adjustDownload,
		&adjustTz))
	{
		m_startServer += static_cast<time_t>(clockDelta);
		if (adjustDownload)
		{
			m_startDownload += static_cast<time_t>(clockDelta);
		}
	}
	if (adjustTz)
	{
		AdjustTimeOffset();
	}

	m_lastCheck = m_curTime;

	CheckQuota();

	if (m_statChanged)
	{
		Save();
	}
}

inline void StatMeter::EnterLeaveStandBy(bool enter)
{
	m_standBy = enter;
	if (enter)
	{
		m_pausedFrom = Util::CurrentTime();
	}
	else
	{
		time_t startDownload = 0;
		if (!m_startDownload.compare_exchange_strong(startDownload, Util::CurrentTime()))
		{
			m_startDownload = (time_t)nzbget_rs_statmeter_calc_resumed_download_start(
				(int64_t)m_startDownload.load(),
				(int64_t)Util::CurrentTime(),
				(int64_t)m_pausedFrom.load());
		}

		m_pausedFrom = 0;
		ResetSpeedStat();
	}
}

inline void StatMeter::CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy)
{
	time_t startServer = m_startServer.load();
	*standBy = m_standBy;
	nzbget_rs_sus_calc_total_stat(
		static_cast<int64>(Util::CurrentTime()),
		static_cast<int64>(startServer),
		static_cast<int64>(m_startDownload.load()),
		static_cast<int64>(m_pausedFrom.load()),
		*standBy ? 1 : 0,
		upTimeSec,
		dnTimeSec);
	*allBytes = m_allBytes;
}

inline int64 StatMeter::CalcCurrentDownloadSpeed()
{
	if (m_standBy)
	{
		return 0;
	}
	return nzbget_rs_sus_calc_current_download_speed(
		0,
		static_cast<int64>(Util::CurrentTime()),
		m_speedStartTime.load(),
		m_speedTotalBytes.load());
}

inline int64 StatMeter::CalcMomentaryDownloadSpeed()
{
	time_t curTime = Util::CurrentTime();
	return nzbget_rs_sus_calc_momentary_download_speed(
		static_cast<int64>(curTime),
		static_cast<int64>(m_curSecTime.load()),
		m_curSecBytes.load());
}

inline void StatMeter::AddSpeedReading(int bytes)
{
	std::lock_guard<std::mutex> guard{m_speedTotalBytesMtx};

	time_t curTime = Util::CurrentTime();
	int64 curSecTime = static_cast<int64>(m_curSecTime.load());
	int64 curSecBytes = m_curSecBytes.load();
	int32_t speedBytes[SPEEDMETER_SLOTS];
	int32_t speedTime[SPEEDMETER_SLOTS];
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		speedBytes[i] = m_speedBytes[i].load();
		speedTime[i] = m_speedTime[i].load();
	}
	int speedBytesIndex = m_speedBytesIndex.load();
	int64 speedTotalBytes = m_speedTotalBytes.load();
	int speedStartTime = m_speedStartTime.load();
	int64 speedCorrection = static_cast<int64>(m_speedCorrection);
	int64 allBytes = m_allBytes.load();

	nzbget_rs_sus_add_speed_reading(
		bytes,
		static_cast<int64>(curTime),
		&curSecTime,
		&curSecBytes,
		speedBytes,
		speedTime,
		&speedBytesIndex,
		&speedTotalBytes,
		&speedStartTime,
		&speedCorrection,
		&allBytes);

	m_curSecTime = static_cast<time_t>(curSecTime);
	m_curSecBytes = curSecBytes;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = speedBytes[i];
		m_speedTime[i] = speedTime[i];
	}
	m_speedBytesIndex = speedBytesIndex;
	m_speedTotalBytes = speedTotalBytes;
	m_speedStartTime = speedStartTime;
	m_speedCorrection = static_cast<time_t>(speedCorrection);
	m_allBytes = allBytes;
}

inline void StatMeter::ResetSpeedStat()
{
	time_t curTime = Util::CurrentTime();
	m_speedStartTime = (int)curTime / SPEEDMETER_SLOTSIZE - 1;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = 0;
		m_speedTime[i] = m_speedStartTime.load();
	}
	m_speedBytesIndex = 0;
	m_speedTotalBytes = 0;
	m_speedCorrection = curTime;
	m_curSecTime =	0;
	m_curSecBytes = 0;
}

inline void StatMeter::LogDebugInfo()
{
	info("   ---------- SpeedMeter");
	int32_t speed = 0, timeDiff = 0;
	nzbget_rs_dqh_statmeter_calc_debug_speed_and_timediff(
		CalcCurrentDownloadSpeed(), (int)Util::CurrentTime(),
		m_speedStartTime.load(), SPEEDMETER_SLOTSIZE, &speed, &timeDiff);
	info("      Speed: %i", speed);
	info("      SpeedStartTime: %i", m_speedStartTime.load());
	info("      SpeedTotalBytes: %" PRIi64, m_speedTotalBytes.load());
	info("      SpeedBytesIndex: %i", m_speedBytesIndex.load());
	info("      AllBytes: %" PRIi64, m_allBytes.load());
	info("      Time: %i", (int)Util::CurrentTime());
	info("      TimeDiff: %i", timeDiff);
	for (int i=0; i < SPEEDMETER_SLOTS; i++)
	{
		info("      Bytes[%i]: %i, Time[%i]: %i", i, m_speedBytes[i].load(), i, m_speedTime[i].load());
	}

	Guard guard(m_volumeMutex);
	int index = 0;
	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		info("      ServerVolume %i", index);
		serverVolume.LogDebugInfo();
		index++;
	}
}

inline void StatMeter::AddServerStats(ServerVolume::Stats stats, int serverId)
{
	Guard guard(m_volumeMutex);
	m_serverVolumes[0].AddStats(stats);
	m_serverVolumes[serverId].AddStats(stats);
	m_statChanged = true;
}

inline GuardedServerVolumes StatMeter::GuardServerVolumes()
{
	GuardedServerVolumes serverVolumes(&m_serverVolumes, &m_volumeMutex);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		ServerVolume::Stats stats;
		stats.bytes = 0;
		stats.articles.failed = 0;
		stats.articles.success = 0;
		serverVolume.AddStats(stats);
	}

	return serverVolumes;
}

inline void StatMeter::Save()
{
	if (!g_Options->GetServerMode())
	{
		return;
	}

	Guard guard(m_volumeMutex);
	g_DiskState->SaveStats(g_ServerPool->GetServers(), &m_serverVolumes);
	m_statChanged = false;
}

inline bool StatMeter::Load(bool* perfectServerMatch)
{
	Guard guard(m_volumeMutex);

	bool ok = g_DiskState->LoadStats(g_ServerPool->GetServers(), &m_serverVolumes, perfectServerMatch);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		serverVolume.CalcSlots(serverVolume.GetDataTime() + g_WorkState->GetLocalTimeOffset());
	}

	return ok;
}

inline void StatMeter::CheckQuota()
{
	if (!nzbget_rs_dqh_statmeter_has_any_quota_configured(
		g_Options->GetDailyQuota(), g_Options->GetMonthlyQuota()))
	{
		return;
	}

	int64 monthBytes, dayBytes;
	CalcQuotaUsage(monthBytes, dayBytes);

	int action = 0;
	bool quotaReached = nzbget_rs_sus_eval_quota_status(
		g_Options->GetMonthlyQuota(),
		g_Options->GetDailyQuota(),
		monthBytes,
		dayBytes,
		g_WorkState->GetQuotaReached() ? 1 : 0,
		&action) != 0;
	if (action == 1)
	{
		warn("Monthly quota reached at %s", *Util::FormatSize(monthBytes));
	}
	else if (action == 2)
	{
		warn("Daily quota reached at %s", *Util::FormatSize(dayBytes));
	}
	else if (action == 3)
	{
		info("Quota lifted");
	}
	g_WorkState->SetQuotaReached(quotaReached);
}

inline void StatMeter::CalcQuotaUsage(int64& monthBytes, int64& dayBytes)
{
	Guard guard(m_volumeMutex);

	ServerVolume totalVolume = m_serverVolumes[0];

	time_t locTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	int elapsedSlots = CalcMonthSlots(totalVolume);
	nzbget_rs_sus_calc_quota_usage(
		static_cast<int64>(locTime),
		totalVolume.GetFirstDay(),
		totalVolume.BytesPerDays()->data(),
		totalVolume.BytesPerDays()->size(),
		elapsedSlots,
		&monthBytes,
		&dayBytes);
	debug("month volume: %i MB", nzbget_rs_dqh_statmeter_bytes_to_mb_i32(monthBytes));
}

inline int StatMeter::CalcMonthSlots(const ServerVolume&)
{
	time_t locCurTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	return nzbget_rs_statmeter_calc_month_slots(
		static_cast<int64_t>(locCurTime), g_Options->GetQuotaStartDay());
}

#endif

#ifndef ARTICLEWRITER_H
#define ARTICLEWRITER_H

class CachedSegmentData final : public SegmentData
{
public:
	CachedSegmentData() {}
	CachedSegmentData(char* data, int size) : m_data(data), m_size(size) {}
	CachedSegmentData(const CachedSegmentData&) = delete;
	CachedSegmentData(CachedSegmentData&& other) :
		m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	CachedSegmentData& operator=(CachedSegmentData&& other);
	~CachedSegmentData() override;
	char* GetData() override { return m_data; }

private:
	char* m_data = nullptr;
	int m_size = 0;

	friend class ArticleCache;
};

class ArticleWriter
{
public:
	// more bytes decoded than the article's range holds (they weren't written)
	bool GetRangeExceeded() { return m_articlePtr > m_articleSize; }
	struct OutputPaths 
	{
	    std::string finalPath;
	    std::string tempPath;
	};
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetInfoName(std::string infoName) { m_infoName = std::move(infoName); }
	void SetFileInfo(FileInfo* fileInfo) { m_fileInfo = fileInfo; }
	void SetArticleInfo(ArticleInfo* articleInfo) { m_articleInfo = articleInfo; }
	void Prepare();
	bool Start(Decoder::EFormat format, const char* filename, int64 fileSize, int64 articleOffset, int articleSize);
	bool Write(char* buffer, int len);
	void Finish(bool success);
	bool GetDuplicate() { return m_duplicate; }
	bool HasCachedBuffer() { return m_articleData.GetData() != nullptr; }
	void LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached);
	std::optional<OutputPaths> SetupOutputFile(DiskFile &outfile,
											std::string_view destDir,
											std::string_view filename,
											std::string_view infoFilename,
											bool directWrite,
											bool cached);
	uint32 ProcessArticles(DiskFile& outfile,
						std::string_view infoFilename,
						std::string_view finalOutputPath,
						bool directWrite,
						bool cached);
	bool CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite);
	bool CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath);
	void ReportCompletionStatus(std::string_view infoFilename);
	void HandlePostProcessing(uint32 crc, 
							std::string_view currentPath, 
							std::string_view originalFilename, 
							std::string_view nzbDestDir);
	bool CompleteFileParts();
	static bool MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir);
	void FlushCache();

private:
	bool IsDupeFallbackArticle() const;
	bool GetSkipDiskWrite();

	FileInfo* m_fileInfo;
	ArticleInfo* m_articleInfo;
	DiskFile m_outFile;
	std::string m_tempFilename;
	std::string m_outputFilename;
	std::string m_resultFilename;
	std::string m_infoName;
	Decoder::EFormat m_format = Decoder::efUnknown;
	CachedSegmentData m_articleData;
	int64 m_articleOffset;
	int m_articleSize;
	int m_articlePtr;
	bool m_duplicate = false;
	bool m_commitError = false;

	bool CreateOutputFile(int64 size);
	void BuildOutputFilename();
	void SetWriteBuffer(DiskFile& outFile, int recSize);
};

class ArticleCache final : public Thread
{
public:
	class FlushGuard
	{
	public:
		FlushGuard(FlushGuard&& other) = default;
		~FlushGuard();
	private:
		Guard m_guard;
		FlushGuard(Mutex& mutex);
		friend class ArticleCache;
	};

	void Run() override;
	void Stop() override;
	CachedSegmentData Alloc(int size);
	bool Realloc(CachedSegmentData* segment, int newSize);
	void Free(CachedSegmentData* segment);
	FlushGuard GuardFlush() { return FlushGuard(m_flushMutex); }
	Guard GuardContent() { return Guard(m_contentMutex); }
	bool GetFlushing() { return m_flushing; }
	size_t GetAllocated() { return m_allocated; }
	bool FileBusy(FileInfo* fileInfo) { return fileInfo == m_fileInfo; }
	void WakeupFlush()
	{
		std::lock_guard<std::mutex> guard(m_allocMutex);
		m_allocCond.notify_all();
	}

private:
	std::atomic<size_t> m_allocated{0};
	FileInfo* m_fileInfo = nullptr;
	std::mutex m_allocMutex;
	std::condition_variable m_allocCond;
	Mutex m_flushMutex;
	Mutex m_contentMutex;
	bool m_flushing = false;

	bool CheckFlush(bool flushEverything);
};

extern ArticleCache* g_ArticleCache;

inline CachedSegmentData::~CachedSegmentData()
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
}

inline CachedSegmentData& CachedSegmentData::operator=(CachedSegmentData&& other)
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
	m_data = other.m_data;
	m_size = other.m_size;
	other.m_data = nullptr;
	other.m_size = 0;
	return *this;
}


inline void ArticleWriter::SetWriteBuffer(DiskFile& outFile, int recSize)
{
	int bufSize = nzbget_rs_qhp_calc_write_buffer_size(g_Options->GetWriteBuffer(), recSize);
	if (bufSize > 0)
	{
		outFile.SetWriteBuffer(bufSize);
	}
}

inline void ArticleWriter::Prepare()
{
	BuildOutputFilename();
	m_resultFilename = m_articleInfo->GetResultFilename()
		? m_articleInfo->GetResultFilename()
		: "";
}

inline bool ArticleWriter::IsDupeFallbackArticle() const
{
	return nzbget_rs_dqh_article_writer_is_dupe_fallback_article(
		m_articleInfo ? 1 : 0,
		m_articleInfo ? m_articleInfo->GetDupeFallbackRound() : 0) != 0;
}

inline bool ArticleWriter::Start(Decoder::EFormat format, const char* filename, int64 fileSize,
	int64 articleOffset, int articleSize)
{
	m_outFile.Close();
	m_format = format;
	m_articleOffset = articleOffset;
	m_articleSize = articleSize > 0 ? articleSize : m_articleInfo->GetSize();
	m_articlePtr = 0;

	// prepare file for writing
	if (m_format == Decoder::efYenc)
	{
		if (g_Options->GetDupeCheck() &&
			m_fileInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			!m_fileInfo->GetNzbInfo()->GetManyDupeFiles())
		{
			bool outputInitialized;
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				outputInitialized = m_fileInfo->GetOutputInitialized();
				if (!g_Options->GetDirectWrite() && !m_fileInfo->GetForceDirectWrite())
				{
					m_fileInfo->SetOutputInitialized(true);
				}
			}

			BString<1024> destFilePath;
			if (filename)
			{
				NzbgetRsBuf dfpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					m_fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, filename);
				destFilePath.Set(dfpBuf.data ? dfpBuf.data : "", (int)dfpBuf.len);
				nzbget_rs_free(dfpBuf);
			}
			bool isOwnOutputFile = false;
			if (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite())
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				isOwnOutputFile = m_fileInfo->GetForceDirectWrite() ||
					(!m_fileInfo->GetOutputFilename().empty() &&
					 FileSystem::SameFilename(m_fileInfo->GetOutputFilename().c_str(), destFilePath));
			}

			if (!outputInitialized && !isOwnOutputFile && filename &&
				FileSystem::FileExists(destFilePath))
			{
				m_duplicate = true;
				return false;
			}
		}

		if (g_Options->GetDirectWrite() && !IsDupeFallbackArticle())
		{
			Guard guard = m_fileInfo->GuardOutputFile();
			if (!m_fileInfo->GetOutputInitialized() ||
				(!GetSkipDiskWrite() && !FileSystem::FileExists(m_outputFilename.c_str())))
			{
				if (!GetSkipDiskWrite() && !CreateOutputFile(fileSize))
				{
					return false;
				}
				m_fileInfo->SetOutputInitialized(true);
			}
		}
	}

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	bool isCritical = g_MemoryMonitor &&
		g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL;

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	// and only yEnc tells it (uuencoded parts all have 0 and overwrote each other,
	// uncached they're appended in order)
	if (g_Options->GetArticleCache() > 0 && !g_Options->GetRawArticle() &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle() && !isCritical)
	{
		m_articleData = g_ArticleCache->Alloc(m_articleSize);

		while (!m_articleData.GetData() && g_ArticleCache->GetFlushing() &&
			!(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL))
		{
			Util::Sleep(5);
			m_articleData = g_ArticleCache->Alloc(m_articleSize);
		}

		if (!m_articleData.GetData())
		{
			detail("Article cache is full, using disk for %s", m_infoName.c_str());
		}
	}

	if (!m_articleData.GetData())
	{
		if (GetSkipDiskWrite())
		{
			return true;
		}

		bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
			m_format == Decoder::efYenc && !IsDupeFallbackArticle();
		const char* outFilename = directWrite ? m_outputFilename.c_str() : m_tempFilename.c_str();
		if (!m_outFile.Open(outFilename, directWrite ? DiskFile::omReadWrite : DiskFile::omWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
				"Could not %s file %s: %s", nzbget_rs_dqh_article_writer_open_verb(directWrite ? 1 : 0), outFilename,
				*FileSystem::GetLastErrorMessage());
			return false;
		}
		SetWriteBuffer(m_outFile, m_articleInfo->GetSize());

		if (directWrite)
		{
			m_outFile.Seek(m_articleOffset);
		}
	}

	return true;
}

inline bool ArticleWriter::GetSkipDiskWrite()
{
	return (m_fileInfo && m_fileInfo->GetNzbInfo() && m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ||
		g_Options->GetSkipWrite();
}

inline bool ArticleWriter::Write(char* buffer, int len)
{
	if (nzbget_rs_awc_article_writer_advance_and_check_range(
		g_Options->GetRawArticle() ? 1 : 0,
		&m_articlePtr,
		len,
		m_articleSize) != 0)
	{
		return true;
	}

	if (!g_Options->GetRawArticle() && m_articleData.GetData())
	{
		memcpy(m_articleData.GetData() + m_articlePtr - len, buffer, len);
		return true;
	}

	if (GetSkipDiskWrite())
	{
		return true;
	}

	return m_outFile.Write(buffer, len) > 0;
}

inline void ArticleWriter::Finish(bool success)
{
	m_outFile.Close();

	if (!success)
	{
		if (!GetSkipDiskWrite())
		{
			FileSystem::DeleteFile(m_tempFilename.c_str());
			FileSystem::DeleteFile(m_resultFilename.c_str());
		}
		return;
	}

	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle();

	if (!g_Options->GetRawArticle())
	{
		if (!directWrite && !m_articleData.GetData() && !GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not rename file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
			FileSystem::DeleteFile(m_tempFilename.c_str());
		}

		if (m_articleData.GetData())
		{
			if (m_articleSize != m_articlePtr)
			{
				g_ArticleCache->Realloc(&m_articleData, m_articlePtr);
			}
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_articleInfo->AttachSegment(std::make_unique<CachedSegmentData>(std::move(m_articleData)), m_articleOffset, m_articlePtr);
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() + 1);
		}
		else
		{
			m_articleInfo->SetSegmentOffset(m_articleOffset);
			m_articleInfo->SetSegmentSize(m_articlePtr);
		}
	}
	else
	{
		// rawmode
		if (!GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not move file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
		}
	}
}

/* creates output file and subdirectores */
inline bool ArticleWriter::CreateOutputFile(int64 size)
{
	bool outExists = FileSystem::FileExists(m_outputFilename.c_str());
	int existingStatus = nzbget_rs_dqh_article_writer_check_existing_output_file(
		outExists ? 1 : 0,
		outExists ? FileSystem::FileSize(m_outputFilename.c_str()) : 0,
		size);
	if (existingStatus == 1)
	{
		// keep existing old file from previous program session
		return true;
	}
	if (existingStatus == 2)
	{
		// delete existing old file from previous program session
		FileSystem::DeleteFile(m_outputFilename.c_str());
	}

	// ensure the directory exist
	BString<1024> destDir;
	destDir.Set(m_outputFilename.c_str(), (int)(FileSystem::BaseFileName(m_outputFilename.c_str()) - m_outputFilename.c_str()));
	CString errmsg;

	if (!FileSystem::ForceDirectories(destDir, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create directory %s: %s", *destDir, *errmsg);
		return false;
	}

	if (!FileSystem::AllocateFile(m_outputFilename.c_str(), size, g_Options->GetArticleCache() == 0, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create file %s: %s", m_outputFilename.c_str(), *errmsg);
		return false;
	}

	return true;
}

inline void ArticleWriter::BuildOutputFilename()
{
	if (!m_fileInfo || !m_articleInfo)
		return;

	const char* tmpDir = g_Options->GetTempDir();
	const char* destDir = m_fileInfo->GetNzbInfo()->GetDestDir();
	const bool directWrite = g_Options->GetDirectWrite();
	const bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	std::optional<Guard> guard;
	if (directWrite || forceDirectWrite)
	{
		guard.emplace(m_fileInfo->GuardOutputFile());
	}
	const std::string& existingOutputFilename = m_fileInfo->GetOutputFilename();

	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};

	NzbgetRsBuf resultBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf outputBuf{};
	int hasOutputFilename = 0;
	int updateFileInfoOutput = 0;
	if (!nzbget_rs_awc_article_writer_build_output_filenames(
		tmpDir,
		destDir,
		PATH_SEPARATOR,
		m_fileInfo->GetId(),
		m_articleInfo->GetPartNumber(),
		directWrite ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		existingOutputFilename.c_str(),
		m_fileInfo->GetOutputInitialized() ? 1 : 0,
		m_fileInfo->GetFilename(),
		nullptr,
		fileExistsCb,
		&resultBuf,
		&tempBuf,
		&outputBuf,
		&hasOutputFilename,
		&updateFileInfoOutput))
	{
		return;
	}

	std::string resultFilename(resultBuf.data ? resultBuf.data : "", resultBuf.len);
	nzbget_rs_free(resultBuf);
	m_articleInfo->SetResultFilename(resultFilename.c_str());

	m_tempFilename.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);

	if (hasOutputFilename)
	{
		m_outputFilename.assign(outputBuf.data ? outputBuf.data : "", outputBuf.len);
		if (updateFileInfoOutput)
		{
			m_fileInfo->SetOutputFilename(m_outputFilename);
		}
	}
	nzbget_rs_free(outputBuf);
}

inline bool ArticleWriter::CompleteFileParts()
{
	debug("Completing file parts");
	m_commitError = false;
	debug("ArticleFilename: %s", m_fileInfo->GetFilename());

	// 1. Gather context & configuration
	std::string nzbDestDir;
	std::string nzbName;
	std::string filename;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_fileInfo->GetNzbInfo()->GetName();
		nzbDestDir = m_fileInfo->GetNzbInfo()->GetDestDir();
		filename = m_fileInfo->GetFilename();
	}

	if (m_outputFilename.empty() && !m_fileInfo->GetOutputFilename().empty())
	{
		m_outputFilename = m_fileInfo->GetOutputFilename();
	}
	bool directWriteOpt = g_Options->GetDirectWrite();
	bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	bool curOutputInit = m_fileInfo->GetOutputInitialized();
	bool hasOwnOut = false;
	bool hasForceDest = false;
	if ((directWriteOpt || forceDirectWrite) && !curOutputInit)
	{
		BString<1024> destPath;
		{
			NzbgetRsBuf dpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbDestDir.c_str(), PATH_SEPARATOR, filename.c_str());
			destPath.Set(dpBuf.data ? dpBuf.data : "", (int)dpBuf.len);
			nzbget_rs_free(dpBuf);
		}
		hasOwnOut = !m_outputFilename.empty() && FileSystem::FileExists(m_outputFilename.c_str());
		hasForceDest = !hasOwnOut && forceDirectWrite && FileSystem::FileExists(destPath);
	}
	int newOutputInit = curOutputInit ? 1 : 0;
	bool directWrite = nzbget_rs_awc_article_writer_resolve_direct_write_ready(
		directWriteOpt ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		curOutputInit ? 1 : 0,
		hasOwnOut ? 1 : 0,
		hasForceDest ? 1 : 0,
		&newOutputInit) != 0;
	if (newOutputInit && !curOutputInit)
	{
		m_fileInfo->SetOutputInitialized(true);
	}
	bool cached = m_fileInfo->GetCachedArticles() > 0;

	std::string fullInfoPath;
	{
		NzbgetRsBuf infoPathBuf = nzbget_rs_dqh_article_writer_format_info_path(
			nzbName.c_str(), PATH_SEPARATOR, filename.c_str());
		if (infoPathBuf.data)
		{
			fullInfoPath.assign(infoPathBuf.data, infoPathBuf.len);
		}
		nzbget_rs_free(infoPathBuf);
	}
	LogStartMessage(fullInfoPath, directWrite, cached);

	// 2. Return if no successful articles
	if (m_fileInfo->GetSuccessArticles() == 0)
	{
		detail("Skipping file parts completion for %s: no successful articles", fullInfoPath.c_str());
		if (!GetSkipDiskWrite())
		{
			CleanupOldData(directWrite, nzbDestDir, "");
		}
		ReportCompletionStatus(fullInfoPath);
		return true;
	}

	// 3. Skip all filesystem work when SkipWrite is active
	if (GetSkipDiskWrite())
	{
		ReportCompletionStatus(fullInfoPath);
		// Update in-memory FileInfo state only
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_fileInfo->SetCrc(0); // CRC not computed in SkipWrite mode
			m_fileInfo->SetOutputFilename("");
		}
		return true;
	}

	// 4. Prepare directory
	CString errmsg;
	if (!FileSystem::ForceDirectories(nzbDestDir.c_str(), errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s", nzbDestDir.c_str(), *errmsg);
		return false;
	}

	// 5. Setup output file
	DiskFile outfile;
	auto pathsOpt = SetupOutputFile(outfile, nzbDestDir, filename, fullInfoPath, directWrite, cached);
	if (!pathsOpt)
	{
		return false;
	}

	const auto& [finalOutputPath, tempDestPath] = *pathsOpt;

	// 6. Process data (Write/Join/Move Articles)
	uint32 crc = ProcessArticles(outfile, fullInfoPath, finalOutputPath, directWrite, cached);
	if (m_commitError)
	{
		outfile.Close();
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not commit all article data for %s", fullInfoPath.c_str());
		return false;
	}

	// 7. Finalize file (Close & Move Temp to Final)
	if (!CommitDiskFile(outfile, tempDestPath, finalOutputPath, directWrite))
	{
		return false;
	}

	// 8. Cleanup (Delete parts or old dirs)
	if (!CleanupOldData(directWrite, nzbDestDir, finalOutputPath))
	{
		return false;
	}

	// 9. Report results
	ReportCompletionStatus(fullInfoPath);

	// 10. Post Processing (Renames, Hardlinks, Moves)
	HandlePostProcessing(crc, finalOutputPath, filename, nzbDestDir);
	return true;
}

inline void ArticleWriter::LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached)
{
	if (g_Options->GetRawArticle())
	{
		detail("Moving articles for %s", infoFilename.data());
	}
	else if (directWrite && cached)
	{
		detail("Writing articles for %s", infoFilename.data());
	}
	else if (directWrite)
	{
		detail("Checking articles for %s", infoFilename.data());
	}
	else
	{
		detail("Joining articles for %s", infoFilename.data());
	}
}

inline std::optional<ArticleWriter::OutputPaths>
ArticleWriter::SetupOutputFile(DiskFile &outfile,
							std::string_view destDir,
							std::string_view filename,
							std::string_view infoFilename,
							bool directWrite,
							bool cached)
{
	struct SetupCtx {
		DiskFile* outfile;
		CString forceDirsErr;
	} ctx{&outfile, {}};

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto makeUniqueCb = [](void*, const char* dir, const char* fname, char* outBuf, size_t outCap) -> size_t {
		CString uniq = FileSystem::MakeUniqueFilename(dir, fname);
		size_t len = uniq.Length();
		if (outBuf && outCap > 0)
		{
			size_t copyLen = len < outCap ? len : outCap - 1;
			memcpy(outBuf, *uniq, copyLen);
			outBuf[copyLen] = '\0';
		}
		return len;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	auto forceDirsCb = [](void* rawCtx, const char* path) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return FileSystem::ForceDirectories(path, c->forceDirsErr) ? 1 : 0;
	};
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto openFileCb = [](void* rawCtx, const char* path, int readWrite) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return c->outfile->Open(path, readWrite ? DiskFile::omReadWrite : DiskFile::omWrite) ? 1 : 0;
	};
	auto createDirCb = [](void*, const char* path) -> int {
		return FileSystem::CreateDirectory(path) ? 1 : 0;
	};

	NzbgetRsBuf finalBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf updatedOutBuf{};
	NzbgetRsBuf parentFailedBuf{};
	NzbgetRsBuf errPathBuf{};
	int hasParentFailed = 0;
	int errKind = 0;
	int ok = nzbget_rs_awc_article_writer_execute_setup_output_file(
		destDir.data(),
		destDir.size(),
		filename.data(),
		filename.size(),
		PATH_SEPARATOR,
		g_Options->GetRawArticle() ? 1 : 0,
		directWrite ? 1 : 0,
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		m_outputFilename.data(),
		m_outputFilename.size(),
		&ctx,
		sameFilenameCb,
		makeUniqueCb,
		fileExistsCb,
		forceDirsCb,
		deleteFileCb,
		openFileCb,
		createDirCb,
		&finalBuf,
		&tempBuf,
		&updatedOutBuf,
		&parentFailedBuf,
		&hasParentFailed,
		&errKind,
		&errPathBuf);

	OutputPaths paths;
	paths.finalPath.assign(finalBuf.data ? finalBuf.data : "", finalBuf.len);
	nzbget_rs_free(finalBuf);
	paths.tempPath.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);
	m_outputFilename.assign(updatedOutBuf.data ? updatedOutBuf.data : "", updatedOutBuf.len);
	nzbget_rs_free(updatedOutBuf);
	std::string parentFailed(parentFailedBuf.data ? parentFailedBuf.data : "", parentFailedBuf.len);
	nzbget_rs_free(parentFailedBuf);
	std::string errPath(errPathBuf.data ? errPathBuf.data : "", errPathBuf.len);
	nzbget_rs_free(errPathBuf);

	if (hasParentFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
			parentFailed.c_str(), *ctx.forceDirsErr);
	}

	if (!ok)
	{
		if (errKind == 1)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 2)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not open file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 3)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		return std::nullopt;
	}

	if (outfile.Active())
	{
		SetWriteBuffer(outfile, 0);
	}

	return paths;
}

inline uint32 ArticleWriter::ProcessArticles(DiskFile& outfile,
							std::string_view infoFilename,
							std::string_view finalOutputPath,
							bool directWrite,
							bool cached)
{
	uint32 crc = 0;
	std::unique_ptr<ArticleCache::FlushGuard> flushGuard;

	if (cached)
	{
		flushGuard = std::make_unique<ArticleCache::FlushGuard>(g_ArticleCache->GuardFlush());
	}

	CharBuffer buffer;
	bool firstArticle = true;

	if (!g_Options->GetRawArticle() && !directWrite)
	{
		buffer.Reserve(1024 * 64);
	}

	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		if (pa->GetStatus() != ArticleInfo::aiFinished) continue;

		// 1. Fill gaps (Direct Write)
		if (!g_Options->GetRawArticle() && !directWrite &&
			pa->GetSegmentOffset() > -1 && pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1)
		{
			memset(buffer, 0, static_cast<size_t>(buffer.Size()));
			if (!GetSkipDiskWrite())
			{
				while (pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1 &&
					outfile.Write(buffer, std::min(pa->GetSegmentOffset() - outfile.Position(), static_cast<int64_t>(buffer.Size()))));
			}
		}

		// 2. Write Content (Cached or Disk)
		if (pa->GetSegmentContent())
		{
			if (!GetSkipDiskWrite())
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while writing cached article %s",
						infoFilename.data());
					continue;
				}
				if (outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize()) !=
					pa->GetSegmentSize())
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not write cached article %s", infoFilename.data());
				}
			}
			pa->DiscardSegment();
		}
		else if (directWrite && pa->GetDupeFallbackRound() > 0 &&
			pa->GetResultFilename() && !GetSkipDiskWrite())
		{
			// Duplicate articles are staged in their per-article temp file even
			// when DirectWrite is enabled.  Copy only after the queue coordinator
			// has accepted the article's final geometry; never let an unvalidated
			// donor overwrite the shared output during download.
			DiskFile infile;
			if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
				FileSystem::FileSize(pa->GetResultFilename()), pa->GetSegmentSize()))
			{
				m_commitError = true;
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Staged duplicate article has an invalid size for %s", infoFilename.data());
			}
			else if (infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while committing staged duplicate article %s",
						infoFilename.data());
					infile.Close();
					continue;
				}
				CharBuffer stagedBuffer(1024 * 64);
				int cnt = 0;
				int64 copied = 0;
				while ((cnt = infile.Read(stagedBuffer, stagedBuffer.Size())) > 0)
				{
					if (outfile.Write(stagedBuffer, cnt) != cnt)
					{
						m_commitError = true;
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not commit staged duplicate article %s", infoFilename.data());
						break;
					}
					copied += cnt;
				}
				if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
					copied, pa->GetSegmentSize()))
				{
					m_commitError = true;
				}
				infile.Close();
			}
			else
			{
				m_commitError = true;
			}
		}
		else if (!g_Options->GetRawArticle() && !directWrite && !GetSkipDiskWrite())
		{
			// Read from partial file and append to outfile
			DiskFile infile;
			if (pa->GetResultFilename() && infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				int64_t cnt = buffer.Size();
				while (cnt == buffer.Size())
				{
					cnt = infile.Read(buffer, buffer.Size());
					outfile.Write(buffer, cnt);
				}
				infile.Close();
			}
			else
			{
				int32_t newFailed = 0, newSuccess = 0;
				nzbget_rs_dqh_article_writer_apply_missing_part_counters(
					m_fileInfo->GetFailedArticles(), m_fileInfo->GetSuccessArticles(),
					&newFailed, &newSuccess);
				m_fileInfo->SetFailedArticles(newFailed);
				m_fileInfo->SetSuccessArticles(newSuccess);
				m_fileInfo->GetNzbInfo()->PrintMessage(
					Message::mkError, "Could not find file %s for %s [%i/%zu]",
					pa->GetResultFilename(), infoFilename.data(),
					pa->GetPartNumber(), m_fileInfo->GetArticles()->size());
			}
		}
		else if (g_Options->GetRawArticle())
		{
			// Move raw files
			NzbgetRsBuf dstBuf = nzbget_rs_dqh_article_writer_format_raw_part_dest(
				finalOutputPath.data(), finalOutputPath.size(), PATH_SEPARATOR, pa->GetPartNumber());
			const char* dstFileName = dstBuf.data ? dstBuf.data : "";
			if (!FileSystem::MoveFile(pa->GetResultFilename(), dstFileName))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					pa->GetResultFilename(), dstFileName, *FileSystem::GetLastErrorMessage());
			}
			nzbget_rs_free(dstBuf);
		}

		// 3. Calculate CRC
		if (m_format == Decoder::efYenc)
		{
			int firstArt = firstArticle ? 1 : 0;
			crc = nzbget_rs_dqh_article_writer_step_file_crc(
				firstArt, crc, pa->GetCrc(), pa->GetSegmentSize(), &firstArt);
			firstArticle = firstArt != 0;
		}
	}

	buffer.Clear();
	return crc;
}

inline bool ArticleWriter::CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite)
{
	if (!outfile.Active()) return true;

	bool flushed = outfile.Flush();
	bool closed = outfile.Close();
	int commitAction = nzbget_rs_dqh_article_writer_eval_commit_post_close(
		flushed ? 1 : 0, closed ? 1 : 0, directWrite ? 1 : 0);
	if (commitAction < 0)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not flush completed output file %s", finalOutputPath.data());
		return false;
	}

	if (commitAction == 1)
	{
		// a failed rename leaves the complete data under its temporary name
		// (as it always has): it is not a data-commit failure, and failing the
		// file would discard data that par-rename/par-check can still use
		if (!FileSystem::MoveFile(tempDestPath.data(), finalOutputPath.data()))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					tempDestPath.data(), finalOutputPath.data(), *FileSystem::GetLastErrorMessage());
		}
	}
	return true;
}

inline bool ArticleWriter::CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath)
{
	std::vector<int> dupeFallbackRounds;
	std::vector<const char*> resultFilenames;
	dupeFallbackRounds.reserve(m_fileInfo->GetArticles()->size());
	resultFilenames.reserve(m_fileInfo->GetArticles()->size());
	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		dupeFallbackRounds.push_back(pa->GetDupeFallbackRound());
		resultFilenames.push_back(pa->GetResultFilename());
	}

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	CString moveErrorMessage;
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto moveFileCb = [](void* ctx, const char* from, const char* to) -> int {
		if (FileSystem::MoveFile(from, to))
		{
			return 1;
		}
		if (ctx)
		{
			*static_cast<CString*>(ctx) = FileSystem::GetLastErrorMessage();
		}
		return 0;
	};
	auto dirEmptyCb = [](void*, const char* dir) -> int {
		return FileSystem::DirEmpty(dir) ? 1 : 0;
	};
	auto removeDirCb = [](void*, const char* dir) -> int {
		return FileSystem::RemoveDirectory(dir) ? 1 : 0;
	};

	NzbgetRsBuf checkedOldDirBuf{};
	NzbgetRsBuf deletedOldDirBuf{};
	int moveFailed = 0;
	int hasCheckedOldDir = 0;
	int hasDeletedOldDir = 0;
	int clearOutputFilename = 0;
	nzbget_rs_awc_article_writer_execute_cleanup_old_data(
		directWrite ? 1 : 0,
		nzbDestDir.data(),
		nzbDestDir.size(),
		finalOutputPath.data(),
		finalOutputPath.size(),
		m_outputFilename.data(),
		m_outputFilename.size(),
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0,
		PATH_SEPARATOR,
		ALT_PATH_SEPARATOR,
		dupeFallbackRounds.data(),
		resultFilenames.data(),
		dupeFallbackRounds.size(),
		&moveErrorMessage,
		sameFilenameCb,
		fileExistsCb,
		deleteFileCb,
		moveFileCb,
		dirEmptyCb,
		removeDirCb,
		&moveFailed,
		&checkedOldDirBuf,
		&hasCheckedOldDir,
		&deletedOldDirBuf,
		&hasDeletedOldDir,
		&clearOutputFilename);

	std::string checkedOldDir(checkedOldDirBuf.data ? checkedOldDirBuf.data : "", checkedOldDirBuf.len);
	nzbget_rs_free(checkedOldDirBuf);
	std::string deletedOldDir(deletedOldDirBuf.data ? deletedOldDirBuf.data : "", deletedOldDirBuf.len);
	nzbget_rs_free(deletedOldDirBuf);

	if (moveFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
			m_outputFilename.c_str(), finalOutputPath.data(), *moveErrorMessage);
	}
	if (hasCheckedOldDir)
	{
		debug("Checking old dir for: %s", m_outputFilename.c_str());
	}
	if (hasDeletedOldDir)
	{
		debug("Deleting old dir: %s", deletedOldDir.c_str());
	}
	if (clearOutputFilename)
	{
		m_fileInfo->SetOutputFilename(nullptr);
	}
	return true;
}

inline void ArticleWriter::ReportCompletionStatus(std::string_view infoFilename)
{
	const int completionKind = nzbget_rs_awc_article_writer_classify_completion_status(
		m_fileInfo->GetTotalArticles(), m_fileInfo->GetSuccessArticles(),
		m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles());
	if (completionKind == 1)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Successfully downloaded %s", infoFilename.data());
	}
	else if (completionKind == 2)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning, "%i of %i article downloads failed for \"%s\"",
			nzbget_rs_dqh_article_writer_calc_total_failed_articles(
				m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles()),
			m_fileInfo->GetTotalArticles(), infoFilename.data());
	}
	else
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Partially downloaded %s", infoFilename.data());
	}
}

inline void ArticleWriter::HandlePostProcessing(uint32 crc,
								std::string_view currentPath,
								std::string_view originalFilename,
								std::string_view nzbDestDir)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_fileInfo->SetCrc(crc);
	m_fileInfo->SetOutputFilename(currentPath.data());

	// 1. Rename if file was renamed during completion
	if (nzbget_rs_dqh_article_writer_was_renamed_during_completion(
		m_fileInfo->GetFilename(), originalFilename.data(), originalFilename.size()) != 0)
	{
		CString newOfn = FileSystem::MakeUniqueFilename(nzbDestDir.data(), m_fileInfo->GetFilename());
		if (!FileSystem::MoveFile(m_fileInfo->GetOutputFilename().c_str(), *newOfn))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not rename file %s to %s: %s",
				m_fileInfo->GetOutputFilename().c_str(), *newOfn, *FileSystem::GetLastErrorMessage());
		}
		m_fileInfo->SetOutputFilename(newOfn);
	}

	// 2. Handle Hardlinks
	if (nzbget_rs_dqh_article_writer_should_delete_hardlinked_without_interdir(
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0) != 0)
	{
		if (FileSystem::DeleteFile(m_outputFilename.c_str()))
		{
			m_fileInfo->SetOutputFilename(nullptr);
			return;
		}
		else
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Cannot remove hardlink");
		}
	}

	// 3. Move Completed Files (if destination changed)
	if (nzbget_rs_dqh_article_writer_did_dest_dir_change(
		m_fileInfo->GetNzbInfo()->GetDestDir(), nzbDestDir.data(), nzbDestDir.size()) != 0)
	{
		MoveCompletedFiles(m_fileInfo->GetNzbInfo(), nzbDestDir.data());
	}
}

inline void ArticleWriter::FlushCache()
{
	detail("Flushing cache for %s", m_infoName.c_str());

	// a file being retried writes into its existing output (ForceDirectWrite), as
	// Start and Finish do: flushed to temporary files instead, its cached articles
	// were never written to the file although marked done
	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_fileInfo->GetOutputInitialized();
	DiskFile outfile;
	bool needBufFile = false;
	int flushedArticles = 0;
	int64 flushedSize = 0;

	{
		ArticleCache::FlushGuard flushGuard = g_ArticleCache->GuardFlush();

		std::vector<ArticleInfo*> cachedArticles;

		{
			Guard contentGuard = g_ArticleCache->GuardContent();

			if (m_fileInfo->GetFlushLocked())
			{
				return;
			}

			m_fileInfo->SetFlushLocked(true);

			cachedArticles.reserve(m_fileInfo->GetArticles()->size());
			for (ArticleInfo* pa : m_fileInfo->GetArticles())
			{
				if (pa->GetSegmentContent())
				{
					cachedArticles.push_back(pa);
				}
			}
		}

		for (ArticleInfo* pa : cachedArticles)
		{
			if (m_fileInfo->GetDeleted() && !m_fileInfo->GetNzbInfo()->GetParking())
			{
				// the file was deleted during flushing: stop flushing immediately
				break;
			}

			if (!GetSkipDiskWrite())
			{
				if (directWrite && !outfile.Active())
				{
					const std::string& outputFilename = m_fileInfo->GetOutputFilename();
					if (!outfile.Open(outputFilename.c_str(), DiskFile::omReadWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not open file %s: %s", outputFilename.c_str(),
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				BString<1024> destFile;

				if (!directWrite)
				{
					NzbgetRsBuf tmpBuf = nzbget_rs_tpr_article_writer_format_tmp_path(pa->GetResultFilename());
					destFile.Set(tmpBuf.data ? tmpBuf.data : "", (int)tmpBuf.len);
					nzbget_rs_free(tmpBuf);
					if (!outfile.Open(destFile, DiskFile::omWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not create file %s: %s", *destFile,
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				if (outfile.Active() && needBufFile)
				{
					SetWriteBuffer(outfile, 0);
					needBufFile = false;
				}

				if (directWrite)
				{
					outfile.Seek(pa->GetSegmentOffset());
				}

				outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize());

				flushedSize += pa->GetSegmentSize();
				flushedArticles++;

				pa->DiscardSegment();

				if (!directWrite)
				{
					outfile.Close();

					if (!FileSystem::MoveFile(destFile, pa->GetResultFilename()))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not rename file %s to %s: %s", *destFile, pa->GetResultFilename(),
							*FileSystem::GetLastErrorMessage());
					}
				}
			}
			else
			{
				flushedSize += pa->GetSegmentSize();
				flushedArticles++;
				pa->DiscardSegment();
			}
		}

		outfile.Close();

		{
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() - flushedArticles);
			m_fileInfo->SetFlushLocked(false);
		}
	}

	double flushedMb = nzbget_rs_dqh_stream_repair_bytes_to_mb(flushedSize);
	if (GetSkipDiskWrite())
	{
		detail("Discarded %i articles (%.2f MB) from cache (SkipWrite) for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
	else
	{
		detail("Saved %i articles (%.2f MB) from cache into disk for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
}

inline bool ArticleWriter::MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir)
{
	if (nzbInfo->GetSkipDiskWrite())
	{
		return true; // nothing was written, nothing to move
	}

	if (nzbInfo->GetCompletedFiles()->empty())
	{
		return true;
	}

	const fs::path destDir = fs::u8path(nzbInfo->GetDestDir());
	const fs::path oldDir  = fs::u8path(oldDestDir);
	fs::error_code ec;
	fs::create_directories(destDir, ec);
	if (ec)
	{
		nzbInfo->PrintMessage(
			Message::mkError,
			"Could not create directory %s: %s",
			nzbInfo->GetDestDir(),
			ec.message().c_str()
		);
		return false;
	}

	// move already downloaded files to new destination
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		const auto fileName = fs::u8path(completedFile.GetFilename());
		const auto oldPath = oldDir / fileName;
		fs::path newPath = destDir / fileName;

		if (oldPath == newPath || !fs::exists(oldPath, ec)) continue;

		if (fs::exists(newPath, ec))
		{
			newPath = fs::make_unique_filename(newPath);
		}

		if (newPath.has_parent_path())
		{
			fs::create_directories(newPath.parent_path(), ec);
		}

		fs::move_file(oldPath, newPath, ec);
		if (ec)
		{
			const auto newPathStr = fs::u8string(newPath);
			nzbInfo->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
				oldPath.string().c_str(), newPathStr.c_str(), ec.message().c_str());
		}
		else
		{
			fs::path oldDirParent = oldDir.parent_path();
			fs::path parent = oldPath.parent_path();
			while (parent != oldDirParent)
			{
				if (fs::exists(parent, ec) && fs::is_empty(parent, ec))
				{
					fs::remove(parent, ec);
					parent = parent.parent_path();
				}
				else
				{
					break;
				}
			}
		}
	}

	if (fs::exists(oldDir, ec) && fs::is_empty(oldDir, ec))
	{
		bool pendingWrites = false;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (pendingWrites) break;

			if (fileInfo->GetOutputInitialized() && !fileInfo->GetOutputFilename().empty() &&
				fileInfo->GetActiveDownloads() > 0)
			{
				Guard guard = fileInfo->GuardOutputFile();
				if (fileInfo->GetOutputInitialized())
				{
					pendingWrites = true;
				}
			}
		}

		if (!pendingWrites)
		{
			fs::remove(oldDir, ec);
		}
	}

	return true;
}

inline CachedSegmentData ArticleCache::Alloc(int size)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = nullptr;
	size_t effectiveLimit = g_MemoryMonitor ?
		g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
		nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

	int wasEmpty = 0;
	if (nzbget_rs_awc_article_cache_can_alloc(effectiveLimit, m_allocated, (size_t)size, &wasEmpty) != 0)
	{
		p = malloc(size);
		if (p)
		{
			if (nzbget_rs_dqh_article_cache_should_write_cache_flag(
				wasEmpty, g_Options->GetServerMode() ? 1 : 0,
				g_Options->GetContinuePartial() ? 1 : 0,
				g_Options->GetSkipWrite() ? 1 : 0) != 0)
			{
				g_DiskState->WriteCacheFlag();
			}
			if (wasEmpty)
			{
				// Resume Run(), the notification arrives later, after releasing m_allocMutex
				m_allocCond.notify_all();
			}
			m_allocated += size;
		}
	}

	return CachedSegmentData((char*)p, p ? size : 0);
}

inline bool ArticleCache::Realloc(CachedSegmentData* segment, int newSize)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = realloc(segment->m_data, newSize);
	if (p)
	{
		m_allocated += newSize - segment->m_size;
		segment->m_size = newSize;
		segment->m_data = (char*)p;
	}

	return p;
}

inline void ArticleCache::Free(CachedSegmentData* segment)
{
	if (segment->m_size)
	{
		free(segment->m_data);

		std::lock_guard<std::mutex> guard(m_allocMutex);
		size_t sub = segment->m_size > 0 ? (size_t)segment->m_size : 0;
		m_allocated = m_allocated >= sub ? (m_allocated - sub) : 0;
		if (m_allocated == 0 && g_Options->GetServerMode() && g_Options->GetContinuePartial())
		{
			g_DiskState->DeleteCacheFlag();
		}
	}
}

inline void ArticleCache::Run()
{
	int resetCounter = 0;
	bool justFlushed = false;
	while (nzbget_rs_dqh_article_cache_should_continue_run_loop(
		IsStopped() ? 1 : 0, m_allocated) != 0)
	{
		size_t effectiveLimit = g_MemoryMonitor ?
			g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
			nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

		int tier = g_MemoryMonitor ? (int)g_MemoryMonitor->GetTier() : (int)NZBGET_RS_PRESSURE_NORMAL;
		int flushEverything = 0;
		int needEmergencyFlush = 0;
		if (nzbget_rs_qhp_article_cache_should_flush(
				m_allocated, effectiveLimit, tier, justFlushed ? 1 : 0, resetCounter,
				IsStopped() ? 1 : 0, g_Options->GetDirectWrite() ? 1 : 0,
				&flushEverything, &needEmergencyFlush) != 0)
		{
			justFlushed = CheckFlush(flushEverything != 0);
			resetCounter = 0;

			int postFlushAction = nzbget_rs_dqh_article_cache_post_flush_action(
				needEmergencyFlush, m_allocated, justFlushed ? 1 : 0, IsStopped() ? 1 : 0);
			if (postFlushAction == 1)
			{
				MemoryMonitor::TrimHeap();
			}
			else if (postFlushAction == 2)
			{
				Util::Sleep(5);
			}
		}
		else if (!m_allocated)
		{
			std::unique_lock<std::mutex> lk(m_allocMutex);
			m_allocCond.wait(lk, [&] { return IsStopped() || m_allocated > 0; });
			resetCounter = 0;
		}
		else
		{
			Util::Sleep(5);
			resetCounter += 5;
		}
	}
}

inline void ArticleCache::Stop()
{
	Thread::Stop();

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_allocMutex);
	m_allocCond.notify_all();
}

inline bool ArticleCache::CheckFlush(bool flushEverything)
{
	debug("Checking cache, Allocated: %i, FlushEverything: %i", (int)m_allocated, (int)flushEverything);

	BString<1024> infoName;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (m_fileInfo)
			{
				break;
			}

			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (nzbget_rs_awc_article_cache_can_flush_file(
						fileInfo->GetCachedArticles(), fileInfo->GetActiveDownloads(),
						fileInfo->GetNzbInfo()->GetSkipDiskWrite() ? 1 : 0,
						flushEverything ? 1 : 0) != 0)
				{
					m_fileInfo = fileInfo;
					NzbgetRsBuf infoBuf = nzbget_rs_dqh_article_writer_format_info_path(
						m_fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, m_fileInfo->GetFilename());
					infoName.Set(infoBuf.data ? infoBuf.data : "", (int)infoBuf.len);
					nzbget_rs_free(infoBuf);
					break;
				}
			}
		}
	}

	if (m_fileInfo)
	{
		ArticleWriter articleWriter;
		articleWriter.SetFileInfo(m_fileInfo);
		articleWriter.SetInfoName(infoName);
		articleWriter.FlushCache();
		m_fileInfo = nullptr;
		return true;
	}

	debug("Checking cache... nothing to flush");

	return false;
}

inline ArticleCache::FlushGuard::FlushGuard(Mutex& mutex) : m_guard(&mutex)
{
	g_ArticleCache->m_flushing = true;
}

inline ArticleCache::FlushGuard::~FlushGuard()
{
	if (m_guard)
	{
		g_ArticleCache->m_flushing = false;
	}
}


inline MemoryMonitor::MemoryMonitor()
{
	InitOptions();
}

inline void MemoryMonitor::InitOptions()
{
	if (g_Options && !Util::EmptyStr(g_Options->GetMemoryPressurePath()))
	{
		nzbget_rs_memory_pressure_set_mock_root(g_Options->GetMemoryPressurePath());
	}
	else if (g_Options)
	{
		nzbget_rs_memory_pressure_set_mock_root(nullptr);
	}
	UpdateState();
}

inline MemoryMonitor::~MemoryMonitor()
{
	if (IsRunning())
	{
		Stop();
	}
	while (IsRunning())
	{
		Util::Sleep(10);
	}
}

inline void MemoryMonitor::Run()
{
	debug("MemoryMonitor thread started");

	while (!IsStopped())
	{
		UpdateState();

		int interval = g_Options ? g_Options->GetMemoryPressureInterval() : 500;
		if (interval < 100) interval = 100;

		int slept = 0;
		while (nzbget_rs_dqh_memory_monitor_should_keep_sleeping(
			IsStopped() ? 1 : 0, slept, interval) != 0)
		{
			Util::Sleep(50);
			slept += 50;
		}
	}

	debug("MemoryMonitor thread stopped");
}

inline void MemoryMonitor::Stop()
{
	Thread::Stop();
}

inline void MemoryMonitor::UpdateState()
{
	int rawSample = nzbget_rs_memory_pressure_sample();
	int currentTier = m_tier.load(std::memory_order_relaxed);
	int nextTier = currentTier;
	bool transitioned = nzbget_rs_qhp_memory_monitor_step(
		rawSample, currentTier, &m_candidateTier, &m_consecutiveSamples, &nextTier) != 0;

	uint32_t cfgCache = g_Options ? (uint32_t)g_Options->GetArticleCache() : 0;
	uint32_t cfgPar = g_Options ? (uint32_t)g_Options->GetParBuffer() : 0;
	uint32_t cfgLimit = 20;
	uint32_t rsEffCache = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_cache(cfgCache) : 0;
	uint32_t rsEffPar = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_par_buffer(cfgPar) : 0;
	uint32_t effCache = 0, effPar = 0, effLimit = 0;
	nzbget_rs_awc_memory_monitor_resolve_tier_limits(
		nextTier,
		rawSample,
		cfgCache,
		cfgPar,
		cfgLimit,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL,
		rsEffCache,
		rsEffPar,
		&effCache,
		&effPar,
		&effLimit);
	m_effectiveCacheMB.store(effCache, std::memory_order_release);
	m_effectiveParBufferMB.store(effPar, std::memory_order_release);
	m_effectiveDownloadsLimit.store(effLimit, std::memory_order_release);

	if (transitioned)
	{
		m_tier.store(nextTier, std::memory_order_release);
		ApplyTierTransition((NzbgetRsPressureTier)currentTier, (NzbgetRsPressureTier)nextTier);
	}
}

inline void MemoryMonitor::ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier)
{
	int action = nzbget_rs_awc_memory_monitor_classify_tier_transition_action(
		(int)oldTier, (int)newTier,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL);
	if (action == 1)
	{
		warn("System memory pressure CRITICAL (>=90%%): shedding article cache and throttling downloads");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
		TrimHeap();
	}
	else if (action == 2)
	{
		detail("System memory pressure CONSTRAINED: shrinking article cache ceiling");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
	}
	else if (action == 3)
	{
		detail("System memory pressure returned to NORMAL: restoring cache and download capacity");
		TrimHeap();
	}
}

#endif

class ArticleFetchLimits
{
public:
	static constexpr int64 MaxDecodedBytes = 64LL * 1024 * 1024;
	static constexpr int64 MaxHeaderBytes = 64LL * 1024;
	static constexpr int64 MaxRawBytes = MaxDecodedBytes * 3 + MaxHeaderBytes;

	bool AddRawBytes(int64 bytes);
	bool HeaderWithinLimit(bool declaredRangeKnown) const;
	bool AddDecodedBytes(int64 bytes, int64 begin, int64 end, int64 fileSize);

private:
	int64 m_rawBytes = 0;
	int64 m_decodedBytes = 0;
};

class ArticleFetcher
{
public:
	struct FetchedArticle
	{
		std::vector<char> Data;	// decoded bytes
		int64 Offset = 0;		// decoded-stream offset (yEnc part begin - 1)
		int64 FileSize = 0;		// total decoded file size from "=ybegin size="
		bool Success = false;
		bool Retry = false;		// interrupted by quota; retry without blaming source
		bool Transient = false;
	};

	static constexpr int MaxTransientAttempts = 4;

	static int TransientRetryDelay(int attempt) { return attempt; }

	FetchedArticle Fetch(const char* messageId, const std::vector<CString>& groups);

	FetchedArticle FetchFromConnection(NntpConnection* connection,
		const char* messageId, const std::vector<CString>& groups);

	void Stop();

	void SetWaitCounter(std::atomic<int>* counter) { m_waitCounter = counter; }

private:
	std::atomic<int>* m_waitCounter = nullptr;
	std::atomic<bool> m_stopped{false};
	Mutex m_connectionMutex;
	NntpConnection* m_connection = nullptr;

	static bool ServerUnreachable(NewsServer* server);
	static void SetServerUnreachable(NewsServer* server, bool unreachable);
	bool WaitBeforeRetry(int seconds);

	void ReleaseConnection(NntpConnection* connection, bool keepConnected);
	void AddServerStats(NntpConnection* connection);
};

class ArticleBatchFetcher
{
public:
	struct Request
	{
		CString MessageId;
		std::shared_ptr<const std::vector<CString>> Groups;
	};
	using FetchFunc = std::function<ArticleFetcher::FetchedArticle(const Request&)>;

	static constexpr int MaxWindowParts = 8;
	static constexpr int64 MaxBufferedBytes = 128LL * 1024 * 1024;

	ArticleBatchFetcher();
	explicit ArticleBatchFetcher(FetchFunc fetchFunc);
	~ArticleBatchFetcher();

	void SetWorkerCount(int workerCount);
	void SetWaitCounter(std::atomic<int>* counter);
	int GetWindowParts() const;
	void Begin(std::vector<Request> requests);
	bool Next(ArticleFetcher::FetchedArticle& result);
	void CancelRemaining();
	void Stop();

private:
	Mutex m_mutex;
	int m_workerCount = 0;
	std::atomic<int>* m_waitCounter = nullptr;
	std::vector<std::unique_ptr<ArticleFetcher>> m_workerFetchers;
	FetchFunc m_fetchFunc;			// test seam; production uses real fetchers
	ArticleFetcher m_inlineFetcher;	// worker count 0 (live mode)
	void* m_rsBatch = nullptr;

	void InitRsBatch();
	void EnsureWorkerFetchers();
};

namespace ArticleFetcherDetail
{
	inline Mutex g_unreachableMutex;
	inline std::set<int> g_unreachableServers; // news server ids
}

inline bool ArticleFetchLimits::AddRawBytes(int64 bytes)
{
	return nzbget_rs_article_fetch_add_raw_bytes(&m_rawBytes, bytes) != 0;
}

inline bool ArticleFetchLimits::HeaderWithinLimit(bool declaredRangeKnown) const
{
	return nzbget_rs_article_fetch_header_within_limit(m_rawBytes, declaredRangeKnown ? 1 : 0) != 0;
}

inline bool ArticleFetchLimits::AddDecodedBytes(int64 bytes, int64 begin, int64 end,
	int64 fileSize)
{
	return nzbget_rs_article_fetch_add_decoded_bytes(&m_decodedBytes, bytes, begin, end, fileSize) != 0;
}

inline ArticleFetcher::FetchedArticle ArticleFetcher::Fetch(const char* messageId,
	const std::vector<CString>& groups)
{
	FetchedArticle result;
	ServerPool::RawServerList failedServers;
	std::map<NewsServer*, int> transientAttempts;
	std::map<NewsServer*, int> staleConnections;

	int level = 0;
	while (level <= g_ServerPool->GetMaxNormLevel() && !m_stopped)
	{
		while (g_WorkState->GetQuotaReached() && !m_stopped)
		{
			Util::Sleep(100);
		}
		if (m_stopped)
		{
			break;
		}
		int eligible = 0;
		std::vector<int> failedIds, failedGroups, failedLevels;
		failedIds.reserve(failedServers.size());
		failedGroups.reserve(failedServers.size());
		failedLevels.reserve(failedServers.size());
		for (NewsServer* failed : failedServers)
		{
			failedIds.push_back(failed->GetId());
			failedGroups.push_back(failed->GetGroup());
			failedLevels.push_back(failed->GetNormLevel());
		}
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_tpr_article_fetcher_is_server_eligible_on_level(
				server->GetActive() ? 1 : 0, server->GetNormLevel(), level,
				server->GetId(), server->GetGroup(),
				failedIds.data(), failedGroups.data(), failedLevels.data(), failedServers.size()) != 0)
			{
				eligible++;
			}
		}
		if (eligible == 0)
		{
			level++;
			continue;
		}

		NntpConnection* connection = nullptr;
		if (m_waitCounter)
		{
			(*m_waitCounter)++;
		}
		while (!connection && !m_stopped)
		{
			if (g_WorkState->GetQuotaReached())
			{
				Util::Sleep(100);
				continue;
			}
			connection = g_ServerPool->GetConnection(level, nullptr, &failedServers);
			if (!connection)
			{
				Util::Sleep(5);
			}
		}
		if (m_waitCounter)
		{
			(*m_waitCounter)--;
		}

		if (!connection)
		{
			level++;
			continue;
		}

		bool stopped = false;
		{
			Guard guard(m_connectionMutex);
			m_connection = connection;
			stopped = m_stopped;
		}
		if (stopped)
		{
			ReleaseConnection(connection, true);
			break;
		}

		bool reused = connection->GetStatus() == Connection::csConnected;
		result = FetchFromConnection(connection, messageId, groups);

		NewsServer* server = connection->GetNewsServer();
		ReleaseConnection(connection, true);

		if (nzbget_rs_dqh_article_fetcher_should_clear_unreachable(
			result.Success ? 1 : 0, result.Transient ? 1 : 0) != 0)
		{
			SetServerUnreachable(server, false);	// it answered
		}
		if (result.Success)
		{
			return result;
		}
		if (result.Retry)
		{
			while (g_WorkState->GetQuotaReached() && !m_stopped)
			{
				Util::Sleep(100);
			}
			continue;
		}
		if (nzbget_rs_dqh_article_fetcher_can_attempt_transient_retry(
			result.Transient ? 1 : 0, ServerUnreachable(server) ? 1 : 0) != 0)
		{
			if (reused && nzbget_rs_tpr_article_fetcher_is_reused_stale_within_max(
				reused ? 1 : 0, ++staleConnections[server], server->GetMaxConnections()) != 0)
			{
				continue;
			}
			int attempt = ++transientAttempts[server];
			if (nzbget_rs_dqh_article_fetcher_has_more_transient_attempts(
				attempt, MaxTransientAttempts) != 0)
			{
				if (WaitBeforeRetry(TransientRetryDelay(attempt)))
				{
					continue;	// ask this server again on a fresh connection
				}
				break;
			}
			SetServerUnreachable(server, true);
		}

		failedServers.push_back(server);
	}

	return result;
}

inline bool ArticleFetcher::ServerUnreachable(NewsServer* server)
{
	Guard guard(ArticleFetcherDetail::g_unreachableMutex);
	return ArticleFetcherDetail::g_unreachableServers.count(server->GetId()) > 0;
}

inline void ArticleFetcher::SetServerUnreachable(NewsServer* server, bool unreachable)
{
	Guard guard(ArticleFetcherDetail::g_unreachableMutex);
	if (unreachable)
	{
		ArticleFetcherDetail::g_unreachableServers.insert(server->GetId());
	}
	else
	{
		ArticleFetcherDetail::g_unreachableServers.erase(server->GetId());
	}
}

inline bool ArticleFetcher::WaitBeforeRetry(int seconds)
{
	for (int waited = 0;
		nzbget_rs_dqh_article_fetcher_should_keep_waiting_retry(
			waited, seconds, m_stopped ? 1 : 0) != 0;
		waited++)
	{
		Util::Sleep(100);
	}
	return !m_stopped;
}

inline void ArticleFetcher::Stop()
{
	m_stopped = true;
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

inline void ArticleFetcher::ReleaseConnection(NntpConnection* connection, bool keepConnected)
{
	Guard guard(m_connectionMutex);
	AddServerStats(connection);
	if (nzbget_rs_dqh_article_fetcher_should_disconnect_on_release(
		keepConnected ? 1 : 0, (int)connection->GetStatus(), (int)Connection::csCancelled) != 0)
	{
		connection->Disconnect();
	}
	if (m_connection == connection)
	{
		m_connection = nullptr;
	}
	g_ServerPool->FreeConnection(connection, true);
}

inline void ArticleFetcher::AddServerStats(NntpConnection* connection)
{
	int bytesRead = connection->FetchTotalBytesRead();
	if (bytesRead <= 0)
	{
		return;
	}

	ServerVolume::Stats stats;
	stats.bytes = Util::SafeIntCast<int, uint32>(bytesRead);
	stats.articles.failed = 0;
	stats.articles.success = 0;
	g_StatMeter->AddServerStats(stats, connection->GetNewsServer()->GetId());
}

inline ArticleFetcher::FetchedArticle ArticleFetcher::FetchFromConnection(NntpConnection* connection,
	const char* messageId, const std::vector<CString>& groups)
{
	FetchedArticle result;
	if (m_stopped)
	{
		return result;
	}

	if (!connection->Connect())
	{
		detail("Stream repair: could not connect to %s", connection->GetNewsServer()->GetName());
		result.Transient = true;
		return result;
	}

	if (connection->GetNewsServer()->GetJoinGroup())
	{
		const char* response = nullptr;
		for (const CString& group : groups)
		{
			response = connection->JoinGroup(group);
			if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) != 0)
			{
				break;
			}
		}
		if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
		{
			detail("Stream repair: could not join groups on %s for %s",
				connection->GetNewsServer()->GetName(), messageId);
			result.Transient = nzbget_rs_dqh_article_fetcher_is_join_group_failure_transient(
				response ? 1 : 0) != 0;
			if (result.Transient)
			{
				connection->Disconnect();
			}
			return result;
		}
	}

	NzbgetRsBuf bodyCmdBuf = nzbget_rs_dqh_article_fetcher_format_body_cmd(messageId);
	BString<1024> bodyCmd;
	bodyCmd.Set(bodyCmdBuf.data ? bodyCmdBuf.data : "", (int)bodyCmdBuf.len);
	nzbget_rs_free(bodyCmdBuf);
	const char* response = connection->Request(bodyCmd);
	if (!response)
	{
		detail("Stream repair: no response from %s for %s", connection->GetNewsServer()->GetName(), messageId);
		connection->Disconnect();
		result.Transient = true;
		return result;
	}
	if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
	{
		detail("Stream repair: article %s not available on %s",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}

	Decoder decoder;
	decoder.Clear();
	decoder.SetCrcCheck(true);
	decoder.SetRawMode(false);

	int readChunk = g_Options->GetArticleReadChunkSize();
	CharBuffer recvBuf(readChunk + 128);	// (decoder slack, see ArticleDownloader)
	ArticleFetchLimits limits;

	while (!m_stopped && !decoder.GetEof())
	{
		if (g_WorkState->GetQuotaReached())
		{
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		while (nzbget_rs_tpr_article_fetcher_should_throttle_bandwidth(
			m_stopped ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			g_WorkState->GetSpeedLimit(),
			g_StatMeter->CalcCurrentDownloadSpeed(),
			g_StatMeter->CalcMomentaryDownloadSpeed()) != 0)
		{
			Util::Sleep(10);
		}
		if (g_WorkState->GetQuotaReached())
		{
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		if (m_stopped)
		{
			break;
		}

		char* buffer;
		int len;
		connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = connection->TryRecv(recvBuf, readChunk);
			buffer = recvBuf;
		}

		if (len <= 0)
		{
			detail("Stream repair: connection to %s lost while fetching %s",
				connection->GetNewsServer()->GetName(), messageId);
			connection->Disconnect();
			result = FetchedArticle();
			result.Transient = true;
			return result;
		}

		g_StatMeter->AddSpeedReading(len);
		if (!limits.AddRawBytes(len))
		{
			detail("Stream repair: article %s from %s exceeded the wire-size limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}

		int decodedLen = decoder.DecodeBuffer(buffer, len);
		bool declaredRangeKnown = nzbget_rs_tpr_article_fetcher_is_declared_range_known(
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()) != 0;
		if (!limits.HeaderWithinLimit(declaredRangeKnown))
		{
			detail("Stream repair: article %s from %s exceeded the yEnc-header limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (declaredRangeKnown && !limits.AddDecodedBytes(decodedLen,
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()))
		{
			detail("Stream repair: article %s from %s exceeded its declared decoded range",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (decodedLen > 0)
		{
			result.Data.insert(result.Data.end(), buffer, buffer + decodedLen);
		}
	}

	if (m_stopped)
	{
		connection->Disconnect();
		return result;
	}

	int64 decOffset = 0;
	int64 decFileSize = 0;
	int decSuccess = 0;
	if (!nzbget_rs_nntp_article_fetcher_eval_decoded_yenc(
		(int)decoder.Check(), (int)Decoder::dsFinished,
		(int)decoder.GetFormat(), (int)Decoder::efYenc,
		decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize(),
		result.Data.size(), &decOffset, &decFileSize, &decSuccess))
	{
		detail("Stream repair: article %s from %s failed decoding",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}
	result.Offset = decOffset;
	result.FileSize = decFileSize;
	result.Success = decSuccess != 0;

	return result;
}

inline ArticleBatchFetcher::ArticleBatchFetcher()
{
	InitRsBatch();
}

inline ArticleBatchFetcher::ArticleBatchFetcher(FetchFunc fetchFunc) :
	m_fetchFunc(std::move(fetchFunc))
{
	InitRsBatch();
}

inline ArticleBatchFetcher::~ArticleBatchFetcher()
{
	nzbget_rs_article_batch_fetcher_free(m_rsBatch);
	m_rsBatch = nullptr;
}

inline void ArticleBatchFetcher::InitRsBatch()
{
	m_rsBatch = nzbget_rs_article_batch_fetcher_new(
		this,
		[](void* raw, size_t workerIdx, const char* msgId,
			const char* const* groups, size_t groupCount,
			NzbgetRsBuf* outData, int64_t* outOffset, int64_t* outFileSize,
			int* outSuccess, int* outRetry, int* outTransient) -> int {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			ArticleFetcher::FetchedArticle fetched;
			try
			{
				auto grpVec = std::make_shared<std::vector<CString>>();
				grpVec->reserve(groupCount);
				for (size_t i = 0; i < groupCount; i++)
				{
					grpVec->emplace_back(groups[i] ? groups[i] : "");
				}
				if (self->m_fetchFunc)
				{
					Request req{CString(msgId ? msgId : ""), grpVec};
					fetched = self->m_fetchFunc(req);
				}
				else
				{
					ArticleFetcher* fetcher = &self->m_inlineFetcher;
					if (workerIdx != SIZE_MAX)
					{
						Guard guard(self->m_mutex);
						if (workerIdx < self->m_workerFetchers.size())
						{
							fetcher = self->m_workerFetchers[workerIdx].get();
						}
					}
					fetched = fetcher->Fetch(msgId ? msgId : "", *grpVec);
				}
			}
			catch (const std::exception& e)
			{
				detail("Batch fetch of %s failed: %s", msgId ? msgId : "", e.what());
				return 0;
			}
			catch (...)
			{
				detail("Batch fetch of %s failed", msgId ? msgId : "");
				return 0;
			}
			*outData = nzbget_rs_alloc_buf(fetched.Data.data(), fetched.Data.size());
			*outOffset = fetched.Offset;
			*outFileSize = fetched.FileSize;
			*outSuccess = fetched.Success ? 1 : 0;
			*outRetry = fetched.Retry ? 1 : 0;
			*outTransient = fetched.Transient ? 1 : 0;
			return 1;
		},
		[](void* raw) {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			try
			{
				Guard guard(self->m_mutex);
				self->m_inlineFetcher.Stop();
				for (auto& wf : self->m_workerFetchers)
				{
					wf->Stop();
				}
			}
			catch (...)
			{
			}
		});
}

inline void ArticleBatchFetcher::SetWorkerCount(int workerCount)
{
	m_workerCount = workerCount;
	nzbget_rs_article_batch_fetcher_set_worker_count(m_rsBatch, workerCount);
}

inline void ArticleBatchFetcher::SetWaitCounter(std::atomic<int>* counter)
{
	Guard guard(m_mutex);
	m_waitCounter = counter;
	m_inlineFetcher.SetWaitCounter(counter);
	for (auto& wf : m_workerFetchers)
	{
		wf->SetWaitCounter(counter);
	}
}

inline int ArticleBatchFetcher::GetWindowParts() const
{
	return nzbget_rs_article_batch_fetcher_get_window_parts(m_rsBatch);
}

inline void ArticleBatchFetcher::EnsureWorkerFetchers()
{
	Guard guard(m_mutex);
	while ((int)m_workerFetchers.size() < m_workerCount)
	{
		m_workerFetchers.push_back(std::make_unique<ArticleFetcher>());
		m_workerFetchers.back()->SetWaitCounter(m_waitCounter);
	}
}

inline void ArticleBatchFetcher::Begin(std::vector<Request> requests)
{
	EnsureWorkerFetchers();
	nzbget_rs_article_batch_fetcher_begin(
		m_rsBatch, requests.size(), &requests,
		[](void* raw, size_t idx, const char** outMidPtr, size_t* outMidLen) -> size_t {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const Request& r = (*reqs)[idx];
			*outMidPtr = r.MessageId ? *r.MessageId : "";
			*outMidLen = r.MessageId.Length();
			return r.Groups ? r.Groups->size() : 0;
		},
		[](void* raw, size_t reqIdx, size_t grpIdx, const char** outGrpPtr, size_t* outGrpLen) {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const CString& g = (*(*reqs)[reqIdx].Groups)[grpIdx];
			*outGrpPtr = g ? *g : "";
			*outGrpLen = g.Length();
		});
}

inline bool ArticleBatchFetcher::Next(ArticleFetcher::FetchedArticle& result)
{
	NzbgetRsBuf dataBuf{nullptr, 0};
	int64_t offset = 0;
	int64_t fileSize = 0;
	int success = 0;
	int retry = 0;
	int transient = 0;
	if (!nzbget_rs_article_batch_fetcher_next(
		m_rsBatch, &dataBuf, &offset, &fileSize, &success, &retry, &transient))
	{
		return false;
	}
	result.Data.assign(dataBuf.data ? dataBuf.data : "", (dataBuf.data ? dataBuf.data : "") + dataBuf.len);
	nzbget_rs_free(dataBuf);
	result.Offset = offset;
	result.FileSize = fileSize;
	result.Success = success != 0;
	result.Retry = retry != 0;
	result.Transient = transient != 0;
	return true;
}

inline void ArticleBatchFetcher::CancelRemaining()
{
	nzbget_rs_article_batch_fetcher_cancel_remaining(m_rsBatch);
}

inline void ArticleBatchFetcher::Stop()
{
	nzbget_rs_article_batch_fetcher_stop(m_rsBatch);
}

#endif
#endif

static const char* FORMATVERSION_SIGNATURE = "nzbget diskstate file version ";
// The queue, history and file-state files are written in upstream nzbget's formats
// (64 and 7), so that an nzbget without duplicate repair still reads them: it ignores
// the trailer that carries this build's own data (stream-repair jobs, recovery
// counters, decoded file sizes). Earlier builds of this branch wrote that data inline
// as formats 65/66 and 8/9: those are still read, once, and rewritten in the new form.
inline const int DISKSTATE_QUEUE_VERSION = 64;
inline const int DISKSTATE_QUEUE_LEGACY_VERSION = 66;
inline const int DISKSTATE_FILE_VERSION = 7;
inline const int DISKSTATE_FILE_LEGACY_VERSION = 9;
// the line that starts this build's data after upstream's in a queue, history or progress file
inline const char* DUPESTATE_TRAILER = "#dupestate 1";
inline const char* UNPACKSTATE_TRAILER = "#unpackstate 1";
// the line that carries a file state's decoded file size after upstream's data
inline const char* DECODED_TRAILER = "#decoded ";
inline const int DISKSTATE_STATS_VERSION = 4;
inline const int DISKSTATE_FEEDS_VERSION = 3;

class StateDiskFile : public DiskFile
{
public:
	int64 PrintLine(const char* format, ...) PRINTF_SYNTAX(2);
	char* ReadLine(char* buffer, int64 size);
	int ScanLine(const char* format, ...) SCANF_SYNTAX(2);
};

static inline void WriteRsBuf(StateDiskFile& outfile, NzbgetRsBuf buf)
{
	if (buf.data && buf.len > 0)
	{
		outfile.Write(buf.data, buf.len);
	}
	nzbget_rs_free(buf);
}


inline int64 StateDiskFile::PrintLine(const char* format, ...)
{
	// a line of any length: one of 1,024 characters or more (a long url sent with
	// appendurl) was written past a fixed buffer, onto the stack, and the state file
	// got garbage (unreadable at the next start) or nzbget crashed (SIGSEGV). The
	// reader cuts a longer line to its buffer.
	va_list ap;
	va_start(ap, format);
	va_list ap2;
	va_copy(ap2, ap);
	int len = vsnprintf(nullptr, 0, format, ap);
	va_end(ap);
	if (len < 0)
	{
		va_end(ap2);
		return 0;
	}
	std::vector<char> str(len + 1);
	vsnprintf(str.data(), str.size(), format, ap2);
	va_end(ap2);

	// a line break inside a value (a name, key or url sent through the API) split
	// the record over two lines, and the whole file was set aside as unreadable at
	// the next start: it is kept as a space
	nzbget_rs_dsp_sanitize_line_breaks(str.data(), (size_t)len);

	// replacing terminating <NULL> with <LF>
	str[len++] = '\n';

	Write(str.data(), len);

	return len;
}

inline char* StateDiskFile::ReadLine(char* buffer, int64 size)
{
	if (!DiskFile::ReadLine(buffer, size))
	{
		return nullptr;
	}

	// remove traling '\n'
	int needsScroll = 0;
	size_t newLen = 0;
	if (nzbget_rs_dsp_inspect_read_line(buffer, &needsScroll, &newLen) != 0)
	{
		if (needsScroll != 0)
		{
			// the line is longer than "size", scroll file position to the end of the line
			for (char skipbuf[1024]; DiskFile::ReadLine(skipbuf, 1024); )
			{
				int skipScroll = 0;
				if (nzbget_rs_dsp_inspect_read_line(skipbuf, &skipScroll, nullptr) == 0 || skipScroll == 0)
				{
					break;
				}
			}
		}

		buffer[newLen] = 0;
	}

	return buffer;
}

/*
* Standard "fscanf" scans beoynd current line if the next line is empty.
* This wrapper fixes that.
*/
inline int StateDiskFile::ScanLine(const char* format, ...)
{
	char line[1024];
	if (!ReadLine(line, sizeof(line)))
	{
		return 0;
	}

	va_list ap;
	va_start(ap, format);
	int res = vsscanf(line, format, ap);
	va_end(ap);

	return res;
}


class StateFile
{
public:
	/* readVersion: the newest version read (default: the one written) */
	StateFile(const char* filename, int formatVersion, bool transactional, int readVersion = 0);
	void Discard();
	void SetFlush(bool flush) { m_flush = flush; }
	bool FileExists();
	StateDiskFile* BeginWrite();
	bool FinishWrite();
	StateDiskFile* BeginRead();
	int GetFileVersion() { return m_fileVersion; }
	const char* GetDestFilename() { return m_destFilename; }

private:
	BString<1024> m_destFilename;
	BString<1024> m_tempFilename;
	int m_formatVersion;
	int m_readVersion;
	bool m_transactional;
	bool m_flush = true;
	int m_fileVersion = 0;
	StateDiskFile m_file;

	int ParseFormatVersion(const char* formatSignature);
};


inline StateFile::StateFile(const char* filename, int formatVersion, bool transactional, int readVersion) :
	m_formatVersion(formatVersion),
	m_readVersion(nzbget_rs_dqh_diskstate_resolve_read_version(readVersion, formatVersion)),
	m_transactional(transactional)
{
	NzbgetRsBuf destBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, filename ? filename : "");
	m_destFilename = destBuf.data ? destBuf.data : "";
	nzbget_rs_free(destBuf);
	if (m_transactional)
	{
		NzbgetRsBuf tempBuf = nzbget_rs_dqh_diskstate_format_new_path(*m_destFilename);
		m_tempFilename = tempBuf.data ? tempBuf.data : "";
		nzbget_rs_free(tempBuf);
	}
	else
	{
		m_tempFilename = *m_destFilename;
	}
}

inline void StateFile::Discard()
{
	FileSystem::DeleteFile(m_destFilename);
	// a temp file left from an interrupted write would be "restored" at the next
	// start as if the write had finished (a partial file, all state set aside)
	if (m_transactional)
	{
		FileSystem::DeleteFile(m_tempFilename);
	}
}

/* Parse signature and return format version number
*/
inline int StateFile::ParseFormatVersion(const char* formatSignature)
{
	return nzbget_rs_dsp_parse_format_version(formatSignature);
}

inline bool StateFile::FileExists()
{
	return nzbget_rs_dqh_diskstate_state_file_exists(
		FileSystem::FileExists(m_destFilename) ? 1 : 0,
		m_transactional ? 1 : 0,
		m_transactional && FileSystem::FileExists(m_tempFilename) ? 1 : 0) != 0;
}

inline StateDiskFile* StateFile::BeginWrite()
{
	if (!m_file.Open(m_tempFilename, StateDiskFile::omWrite))
	{
		error("Error saving diskstate: Could not create file %s: %s", *m_tempFilename,
			*FileSystem::GetLastErrorMessage());
		return nullptr;
	}

	m_file.PrintLine("%s%i", FORMATVERSION_SIGNATURE, m_formatVersion);

	return &m_file;
}

inline bool StateFile::FinishWrite()
{
	if (!m_transactional)
	{
		m_file.Close();
		return true;
	}

	// flush file content before renaming
	if (nzbget_rs_dqh_diskstate_should_flush_queue_file(
		m_flush ? 1 : 0, g_Options->GetFlushQueue() ? 1 : 0) != 0)
	{
		debug("Flushing data for file %s", FileSystem::BaseFileName(m_tempFilename));
		m_file.Flush();
		CString errmsg;
		if (!m_file.Sync(errmsg))
		{
			warn("Could not flush file %s into disk: %s", *m_tempFilename, *errmsg);
		}
	}

	m_file.Close();

	// now rename to dest file name
	FileSystem::DeleteFile(m_destFilename);
	if (!FileSystem::MoveFile(m_tempFilename, m_destFilename))
	{
		error("Error saving diskstate: Could not rename file %s to %s: %s",
			*m_tempFilename, *m_destFilename, *FileSystem::GetLastErrorMessage());
		return false;
	}

	// flush directory buffer after renaming
	if (nzbget_rs_dqh_diskstate_should_flush_queue_file(
		m_flush ? 1 : 0, g_Options->GetFlushQueue() ? 1 : 0) != 0)
	{
		debug("Flushing directory for file %s", FileSystem::BaseFileName(m_destFilename));
		CString errmsg;
		if (!FileSystem::FlushDirBuffers(m_destFilename, errmsg))
		{
			warn("Could not flush directory buffers for file %s into disk: %s", *m_destFilename, *errmsg);
		}
	}

	return true;
}

inline StateDiskFile* StateFile::BeginRead()
{
	// with the dest-file there, a temp-file is what an interrupted write left (the
	// dest-file is deleted only right before the temp-file takes its place)
	int recoveryMode = nzbget_rs_dqh_diskstate_classify_begin_read_recovery(
		m_transactional ? 1 : 0,
		FileSystem::FileExists(m_destFilename) ? 1 : 0,
		FileSystem::FileExists(m_tempFilename) ? 1 : 0);
	if (recoveryMode == 1)
	{
		FileSystem::DeleteFile(m_tempFilename);
	}
	else if (recoveryMode == 2)
	{
		// disaster recovery: temp-file exists but the dest-file doesn't
		warn("Restoring diskstate file %s from %s", FileSystem::BaseFileName(m_destFilename), FileSystem::BaseFileName(m_tempFilename));
		if (!FileSystem::MoveFile(m_tempFilename, m_destFilename))
		{
			error("Error restoring diskstate: Could not rename file %s to %s: %s",
				*m_tempFilename, *m_destFilename, *FileSystem::GetLastErrorMessage());
			return nullptr;
		}
	}

	if (!m_file.Open(m_destFilename, StateDiskFile::omRead))
	{
		error("Error reading diskstate: could not open file %s: %s", *m_destFilename,
			*FileSystem::GetLastErrorMessage());
		return nullptr;
	}

	char FileSignatur[128];
	m_file.ReadLine(FileSignatur, sizeof(FileSignatur));
	m_fileVersion = ParseFormatVersion(FileSignatur);
	if (nzbget_rs_dqh_diskstate_is_file_version_unsupported(m_fileVersion, m_readVersion) != 0)
	{
		error("Could not load diskstate file %s due to file version mismatch", *m_destFilename);
		m_file.Close();
		return nullptr;
	}

	return &m_file;
}


/* Save Download Queue to Disk.
 * The Disk State consists of file "queue", which contains the order of files,
 * and of one diskstate-file for each file in download queue.
 * This function saves file "queue" and files with NZB-info. It does not
 * save file-infos.
 */
inline bool DiskState::SaveDownloadQueue(DownloadQueue* downloadQueue, bool saveHistory)
{
	debug("Saving queue and history to disk");

	bool ok = true;

	{
		StateFile stateFile("queue", DISKSTATE_QUEUE_VERSION, true);
		if (!downloadQueue->GetQueue()->empty())
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			// save nzb-infos
			SaveQueue(downloadQueue->GetQueue(), *outfile);

			// now rename to dest file name
			ok = stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	if (saveHistory)
	{
		StateFile stateFile("history", DISKSTATE_QUEUE_VERSION, true);
		if (!downloadQueue->GetHistory()->empty())
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			// save history
			SaveHistory(downloadQueue->GetHistory(), *outfile);

			// now rename to dest file name
			ok &= stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	// progress-file isn't needed after saving of full queue data
	StateFile progressStateFile("progress", DISKSTATE_QUEUE_VERSION, true);
	progressStateFile.Discard();

	SaveIdFloor();



	return ok;
}

inline bool DiskState::LoadDownloadQueue(DownloadQueue* downloadQueue, Servers* servers)
{
	debug("Loading queue from disk");

	bool ok = false;
	bool queueFilesFailed = false;
	bool fileInfosLoaded = false;
	int formatVersion = 0;

	// formats 65/66 are read as an earlier build of this branch wrote them (it may
	// run again after this one); a file that doesn't read so, or of a newer format,
	// is set aside below rather than lost to the next save
	int queueReadVersion = DISKSTATE_QUEUE_LEGACY_VERSION;

	{
		StateFile stateFile("queue", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			formatVersion = stateFile.GetFileVersion();
			int isLegacy = 0;
			int loadInlineHistory = 0;
			int verStatus = nzbget_rs_dsp_check_queue_format_version(
				formatVersion, DISKSTATE_QUEUE_VERSION, &isLegacy, &loadInlineHistory);
			m_legacyQueueRead |= isLegacy != 0;
			if (verStatus == 1)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}
			else if (verStatus == 2)
			{
				error("Failed to read queue and history data. Only queue and history from NZBGet v13 or newer can be converted by this RustyGet version. "
					"Old queue and history data still can be converted using NZBGet v16 as an intermediate version.");
				{ queueFilesFailed = true; goto error; }
			}

			if (!LoadQueue(downloadQueue->GetQueue(), servers, *infile, formatVersion)) { queueFilesFailed = true; goto error; }

			if (loadInlineHistory != 0)
			{
				if (!LoadHistory(downloadQueue->GetHistory(), servers, *infile, formatVersion)) { queueFilesFailed = true; goto error; }
			}
		}
	}

	{
		StateFile stateFile("progress", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			if (stateFile.GetFileVersion() <= 0)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}

			m_legacyQueueRead |= stateFile.GetFileVersion() != DISKSTATE_QUEUE_VERSION;
			if (!LoadProgress(downloadQueue->GetQueue(), servers, *infile, stateFile.GetFileVersion())) { queueFilesFailed = true; goto error; }
		}
	}

	if (nzbget_rs_dqh_diskstate_should_load_separate_history_file(formatVersion) != 0)
	{
		StateFile stateFile("history", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			if (stateFile.GetFileVersion() <= 0)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}

			m_legacyQueueRead |= stateFile.GetFileVersion() != DISKSTATE_QUEUE_VERSION;
			if (!LoadHistory(downloadQueue->GetHistory(), servers, *infile, stateFile.GetFileVersion())) { queueFilesFailed = true; goto error; }
		}
	}

	LoadAllFileInfos(downloadQueue);
	fileInfosLoaded = true;

	CleanupQueueDir(downloadQueue);

	if (!LoadAllFileStates(downloadQueue, servers)) goto error;

	// file states an earlier build of this branch wrote (formats 8/9) are rewritten
	// in the upstream format, history items' too, which are otherwise only rewritten
	// when touched: after a downgrade upstream could not retry such an item
	ConvertLegacyFileStates(servers);

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for download queue and history");
	}
	if (nzbget_rs_dqh_diskstate_should_reload_fallback_file_infos(
		fileInfosLoaded ? 1 : 0, downloadQueue->GetQueue()->empty() ? 1 : 0) != 0)
	{
		// the downloads that did load keep running: with only the stubs the queue
		// file holds (no name, size or groups) they showed blank and the next save
		// wrote the stubs over their file infos
		LoadAllFileInfos(downloadQueue);
	}
	if (queueFilesFailed)
	{
		// the next save would overwrite them: kept under another name instead
		SetAsideQueueFiles();
	}
	RaiseIdsPastQueueDir();

	NzbInfo::ResetGenId(true);
	FileInfo::ResetGenId(true);

	CalcFileStats(downloadQueue, formatVersion);

	return ok;
}

/*
 * The ids the queue dir holds files for: new ids are counted on past them. With the
 * queue and history set aside as unreadable, the ids restarted at the few items that
 * loaded: new items got the ids of old ones and took over their logs, file states
 * and dupe-search state (P0-c).
 */
inline void DiskState::RaiseIdsPastQueueDir()
{
	// the last ids handed out: a download deleted for good leaves no file behind,
	// and without them its id went to the next download after a restart (a client
	// that still knew the old id then edited or deleted the new download)
	CharBuffer ids;
	NzbgetRsBuf idsPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "ids");
	BString<1024> idsFilename = idsPathBuf.data ? idsPathBuf.data : "";
	nzbget_rs_free(idsPathBuf);
	if (FileSystem::FileExists(idsFilename) && FileSystem::LoadFileIntoBuffer(idsFilename, ids, true))
	{
		int nzbId = 0;
		int fileId = 0;
		if (nzbget_rs_dsp_parse_ids_file(ids, &nzbId, &fileId))
		{
			NzbInfo::RaiseIdFloor(nzbId);
			FileInfo::RaiseIdFloor(fileId);
		}
	}

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		int isNzbLog = 0;
		int id = nzbget_rs_dsp_classify_queue_dir_id(filename, &isNzbLog);
		if (id > 0)
		{
			if (isNzbLog)
			{
				NzbInfo::RaiseIdFloor(id);
			}
			else
			{
				FileInfo::RaiseIdFloor(id);
			}
		}
	}
}

/*
 * Keeps the last ids handed out in the queue dir (file "ids", which older
 * versions ignore), for RaiseIdsPastQueueDir.
 */
inline void DiskState::SaveIdFloor()
{
	nzbget_rs_diskstate_save_id_floor(
		g_Options->GetQueueDir(), PATH_SEPARATOR, NzbInfo::LastGenId(), FileInfo::LastGenId());
}

inline bool DiskState::SaveDownloadProgress(DownloadQueue* downloadQueue)
{
	int count = 0;
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		count += nzbInfo->GetChanged() ? 1 : 0;
	}

	debug("Saving queue progress to disk");

	bool ok = true;

	{
		StateFile stateFile("progress", DISKSTATE_QUEUE_VERSION, true);
		if (count > 0)
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			SaveProgress(downloadQueue->GetQueue(), *outfile, count);

			// now rename to dest file name
			ok = stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	return ok;
}

inline void DiskState::ConvertLegacyFileStates(Servers* servers)
{
	// "<id>" (file info), "<id>s" (partial state), "<id>c" (state of a completed file)
	std::vector<std::pair<int, std::string>> legacy;
	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		if (nzbget_rs_dqh_diskstate_is_candidate_queue_file_name(filename) == 0)
		{
			continue;
		}
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
		BString<1024> path = pathBuf.data ? pathBuf.data : "";
		nzbget_rs_free(pathBuf);
		DiskFile file;
		char header[100] = {0};
		if (!file.Open(path, DiskFile::omRead) || !file.ReadLine(header, sizeof(header)))
		{
			continue;
		}
		file.Close();
		int legacyId = 0;
		int suffixKind = 0;
		if (nzbget_rs_dsp_classify_legacy_file_state(
			filename, header, DISKSTATE_FILE_VERSION, DISKSTATE_FILE_LEGACY_VERSION, &legacyId, &suffixKind))
		{
			legacy.emplace_back(legacyId, nzbget_rs_dqh_diskstate_legacy_suffix_str(suffixKind));
		}
	}
	if (legacy.empty())
	{
		return;
	}

	// each file is written anew and renamed over the old one: a file that another user
	// owns (an earlier nzbget run under another account) can't be opened for writing,
	// but in a writable QueueDir it can be replaced. Not flushed one by one: thousands
	// of syncs would hold up the start for minutes.
	int converted = 0;
	for (const auto& entry : legacy)
	{
		FileInfo fileInfo;
		fileInfo.SetId(entry.first);
		bool ok;
		if (entry.second.empty())
		{
			ok = LoadFile(&fileInfo, true, true) && SaveFile(&fileInfo, true);
		}
		else
		{
			bool completed = entry.second == "c";
			ok = LoadFile(&fileInfo, false, true) && LoadFileState(&fileInfo, servers, completed) &&
				SaveFileState(&fileInfo, completed, true);
		}
		converted += ok ? 1 : 0;
	}
	info("Converted %i of %i file state(s) of an earlier build to the upstream format", converted, (int)legacy.size());
}

inline void DiskState::SetAsideQueueFiles()
{
	time_t now = Util::CurrentTime();
	for (const char* name : { "queue", "history", "progress" })
	{
		NzbgetRsBuf fnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		BString<1024> filename = fnBuf.data ? fnBuf.data : "";
		nzbget_rs_free(fnBuf);
		if (FileSystem::FileExists(filename))
		{
			NzbgetRsBuf keptBuf = nzbget_rs_dqh_diskstate_format_unreadable_path(*filename, (int64)now);
			BString<1024> keptName = keptBuf.data ? keptBuf.data : "";
			nzbget_rs_free(keptBuf);
			if (FileSystem::MoveFile(filename, keptName))
			{
				error("Diskstate file %s could not be read; it is kept as %s", name,
					FileSystem::BaseFileName(keptName));
			}
		}
	}
}

inline void DiskState::SaveQueue(NzbList* queue, StateDiskFile& outfile)
{
	debug("Saving nzb list to disk");

	outfile.PrintLine("%i", (int)queue->size());
	for (NzbInfo* nzbInfo : queue)
	{
		SaveNzbInfo(nzbInfo, outfile);
	}

	std::vector<NzbInfo*> nzbs;
	for (NzbInfo* nzbInfo : queue)
	{
		nzbs.push_back(nzbInfo);
	}
	SaveDupeTrailer(nzbs, outfile);
}

inline bool DiskState::LoadQueue(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading nzb list from disk");

	// load nzb-infos
	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
		queue->push_back(std::move(nzbInfo));
	}

	// (before format 57 the history follows in the same file: no trailer there)
	if (formatVersion >= 57 && formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([queue](int id) { return queue->Find(id); }, infile)) goto error;

	return true;

error:
	error("Error reading nzb list from disk");
	return false;
}

inline void DiskState::SaveProgress(NzbList* queue, StateDiskFile& outfile, int changedCount)
{
	debug("Saving nzb progress to disk");

	outfile.PrintLine("%i", changedCount);
	for (NzbInfo* nzbInfo : queue)
	{
		if (nzbInfo->GetChanged())
		{
			outfile.PrintLine("%i", nzbInfo->GetId());
			SaveNzbInfo(nzbInfo, outfile);
		}
	}

	std::vector<NzbInfo*> changed;
	for (NzbInfo* nzbInfo : queue)
	{
		if (nzbInfo->GetChanged())
		{
			changed.push_back(nzbInfo);
		}
	}
	SaveDupeTrailer(changed, outfile);
}

inline bool DiskState::LoadProgress(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading nzb progress from disk");

	// load nzb-infos
	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		int id;
		if (infile.ScanLine("%i", &id) != 1) goto error;

		NzbInfo* nzbInfo = queue->Find(id);
		if (!nzbInfo)
		{
			// a progress record of a collection no longer queued (the queue was saved,
			// then nzbget stopped before the progress file was discarded): read past it.
			// It aborted the whole load, and queue and history were set aside
			warn("Skipping saved progress of NZB with id %i, which is no longer queued", id);
			NzbInfo stale;
			if (!LoadNzbInfo(&stale, servers, infile, formatVersion)) goto error;
			continue;
		}

		if (!LoadNzbInfo(nzbInfo, servers, infile, formatVersion)) goto error;
	}

	if (formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([queue](int id) { return queue->Find(id); }, infile)) goto error;

	return true;

error:
	error("Error reading nzb progress from disk");
	return false;
}

inline void DiskState::SaveDupeExtras(NzbInfo* nzbInfo, StateDiskFile& outfile)
{
	const StreamRepairJobList* repairJobs = nzbInfo->GetStreamRepairJobs();
	std::vector<NzbgetRsDiskStreamHole> allHoles;
	std::vector<NzbgetRsDiskRepairJob> jobs;
	jobs.reserve(repairJobs->size());
	size_t totalHoles = 0;
	for (const StreamRepairJob& job : *repairJobs)
	{
		totalHoles += job.GetHoles()->size();
	}
	allHoles.reserve(totalHoles);

	for (const StreamRepairJob& job : *repairJobs)
	{
		const StreamRangeList* holes = job.GetHoles();
		size_t startIdx = allHoles.size();
		for (const StreamRange& hole : *holes)
		{
			allHoles.push_back(NzbgetRsDiskStreamHole{ hole.Offset, hole.Size });
		}
		jobs.push_back(NzbgetRsDiskRepairJob{
			job.GetFileId(),
			job.GetDecodedFileSize(),
			job.GetFailedSize(),
			job.GetMissedSize(),
			job.GetParFile() ? 1 : 0,
			job.GetFailedArticles(),
			job.GetStepsHash(),
			job.GetFilename(),
			holes->empty() ? nullptr : &allHoles[startIdx],
			holes->size()
		});
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_dupe_extras(
		nzbInfo->GetDupeRecoveredArticles(),
		nzbInfo->GetDupeRecoveredBytes(),
		nzbInfo->GetDupeRecoveredHoles(),
		jobs.data(),
		jobs.size()));
}

inline bool DiskState::LoadDupeExtras(NzbInfo* nzbInfo, StateDiskFile& infile, int formatVersion)
{
	char buf[1024];
	uint32 recoveredHigh = 0, recoveredLow = 0;
	int recoveredArticles = 0, recoveredHoles = 0;
	int headerScanCnt;
	headerScanCnt = infile.ScanLine("%i,%u,%u,%i", &recoveredArticles, &recoveredHigh, &recoveredLow,
		&recoveredHoles);
	if (headerScanCnt != 4 || !nzbget_rs_dsp_validate_dupe_extras_header(
		headerScanCnt, recoveredArticles, recoveredHoles)) goto error;
	nzbInfo->SetDupeRecoveredArticles(recoveredArticles);
	nzbInfo->SetDupeRecoveredBytes(Util::JoinInt64(recoveredHigh, recoveredLow));
	nzbInfo->SetDupeRecoveredHoles(recoveredHoles);

	int repairCount;
	{
		repairCount = 0;
		int repairScanCnt = infile.ScanLine("%i", &repairCount);
		if (repairScanCnt != 1 || repairCount < 0 || repairCount > 100000) goto error;
	}
	for (int i = 0; i < repairCount; i++)
	{
		int fileId = 0, parFile = 0, failedArticles = 0;
		uint32 decodedHigh = 0, decodedLow = 0, failedHigh = 0, failedLow = 0, missedHigh = 0, missedLow = 0;
		uint32 stepsHigh = 0, stepsLow = 0;
		int expectedJobFields = nzbget_rs_dqh_diskstate_expected_repair_job_fields(formatVersion);
		int jobScanCnt = expectedJobFields == 11 ?
			infile.ScanLine("%i,%u,%u,%u,%u,%u,%u,%i,%i,%u,%u", &fileId, &decodedHigh,
				&decodedLow, &failedHigh, &failedLow, &missedHigh, &missedLow, &parFile,
				&failedArticles, &stepsHigh, &stepsLow) :
			infile.ScanLine("%i,%u,%u,%u,%u,%u,%u,%i,%i", &fileId, &decodedHigh,
				&decodedLow, &failedHigh, &failedLow, &missedHigh, &missedLow, &parFile,
				&failedArticles);
		if (jobScanCnt != expectedJobFields || !nzbget_rs_dsp_validate_repair_job_header(
			jobScanCnt, expectedJobFields, fileId, parFile, failedArticles)) goto error;
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		int holeCount;
		{
			holeCount = 0;
			int holeScanCnt = infile.ScanLine("%i", &holeCount);
			if (holeScanCnt != 1 || holeCount < 0 || holeCount > 1000000) goto error;
		}
		int64 decodedSize = Util::JoinInt64(decodedHigh, decodedLow);
		StreamRangeList holes;
		holes.reserve(holeCount);
		for (int j = 0; j < holeCount; j++)
		{
			uint32 offsetHigh, offsetLow, sizeHigh, sizeLow;
			if (infile.ScanLine("%u,%u,%u,%u", &offsetHigh, &offsetLow, &sizeHigh, &sizeLow) != 4) goto error;
			int64 offset = 0;
			int64 size = 0;
			if (!nzbget_rs_dsp_validate_repair_hole(offsetHigh, offsetLow, sizeHigh, sizeLow, decodedSize, &offset, &size)) goto error;
			holes.push_back({offset, size});
		}
		nzbInfo->GetStreamRepairJobs()->emplace_back(fileId, buf, decodedSize,
			Util::JoinInt64(failedHigh, failedLow), Util::JoinInt64(missedHigh, missedLow),
			failedArticles, parFile != 0, std::move(holes),
			(uint64)Util::JoinInt64(stepsHigh, stepsLow));
	}


	return true;

error:
	return false;
}

// this build's data after upstream's: the extras of the nzbs that have any
inline void DiskState::SaveDupeTrailer(const std::vector<NzbInfo*>& nzbs, StateDiskFile& outfile)
{
	std::vector<NzbInfo*> withExtras;
	for (NzbInfo* nzbInfo : nzbs)
	{
		if (nzbget_rs_dsp_has_dupe_extras(
			nzbInfo->GetDupeRecoveredArticles(),
			nzbInfo->GetDupeRecoveredBytes(),
			nzbInfo->GetDupeRecoveredHoles(),
			nzbInfo->GetStreamRepairJobs()->size()))
		{
			withExtras.push_back(nzbInfo);
		}
	}
	outfile.PrintLine("%s", DUPESTATE_TRAILER);
	outfile.PrintLine("%i", (int)withExtras.size());
	for (NzbInfo* nzbInfo : withExtras)
	{
		outfile.PrintLine("%i", nzbInfo->GetId());
		SaveDupeExtras(nzbInfo, outfile);
	}

	outfile.PrintLine("%s", UNPACKSTATE_TRAILER);
	outfile.PrintLine("%i", (int)nzbs.size());
	for (NzbInfo* nzbInfo : nzbs)
	{
		std::vector<const char*> archivePtrs;
		std::vector<CString>* archives = nzbInfo->GetExtractedArchives();
		if (archives)
		{
			archivePtrs.reserve(archives->size());
			for (const CString& arc : *archives)
			{
				archivePtrs.push_back(*arc ? *arc : "");
			}
		}
		WriteRsBuf(outfile, nzbget_rs_diskstate_format_unpack_trailer_item(
			nzbInfo->GetId(),
			static_cast<int>(nzbInfo->GetDirectUnpackStatus()),
			nzbInfo->GetUnpackTried() ? 1 : 0,
			nzbInfo->GetParRepaired() ? 1 : 0,
			nzbInfo->GetLastUnpackStatus(),
			archivePtrs.data(),
			archivePtrs.size()));
	}
}

inline bool DiskState::LoadDupeTrailer(const std::function<NzbInfo*(int)>& find, StateDiskFile& infile)
{
	char buf[1024];
	if (!infile.ReadLine(buf, sizeof(buf)) || !nzbget_rs_dqh_bytes_equal(buf, DUPESTATE_TRAILER))
	{
		return true;	// a file upstream nzbget wrote, or one without any
	}
	int count = 0;
	int countScanCnt = infile.ScanLine("%i", &count);
	if (nzbget_rs_dqh_diskstate_is_nonneg_count_scan_invalid(countScanCnt, count) != 0) goto error;
	for (int i = 0; i < count; i++)
	{
		int id;
		if (infile.ScanLine("%i", &id) != 1) goto error;
		NzbInfo* nzbInfo = find(id);
		NzbInfo unknown;	// an nzb no longer listed: its extras are read and dropped
		if (!LoadDupeExtras(nzbInfo ? nzbInfo : &unknown, infile, DISKSTATE_QUEUE_LEGACY_VERSION)) goto error;
	}
	if (infile.ReadLine(buf, sizeof(buf)) && nzbget_rs_dqh_bytes_equal(buf, UNPACKSTATE_TRAILER))
	{
		int unpackCount = 0;
		int unpackScanCnt = infile.ScanLine("%i", &unpackCount);
		if (nzbget_rs_dqh_diskstate_is_nonneg_count_scan_invalid(unpackScanCnt, unpackCount) != 0) goto error;
		for (int i = 0; i < unpackCount; i++)
		{
			int id = 0;
			if (infile.ScanLine("%i", &id) != 1) goto error;
			NzbInfo* nzbInfo = find(id);
			NzbInfo unknown;
			NzbInfo* target = nzbInfo ? nzbInfo : &unknown;

			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
			char line2[1024];
			char line3[1024];
			if (!infile.ReadLine(line2, sizeof(line2))) goto error;
			if (!infile.ReadLine(line3, sizeof(line3))) goto error;
			int directUnpackStatus = 0, unpackTriedBool = 0, parRepairedBool = 0, lastUnpackStatus = 0;
			if (!nzbget_rs_dsp_parse_unpack_state_lines(
				buf, line2, line3,
				static_cast<int>(NzbInfo::nsRunning), static_cast<int>(NzbInfo::nsNone),
				&directUnpackStatus, &unpackTriedBool, &parRepairedBool, &lastUnpackStatus)) goto error;
			target->SetDirectUnpackStatus(static_cast<NzbInfo::EDirectUnpackStatus>(directUnpackStatus));
			target->SetUnpackTried(unpackTriedBool != 0);
			target->SetParRepaired(parRepairedBool != 0);
			target->SetLastUnpackStatus(lastUnpackStatus);

			std::vector<char> largeBuf(65536);
			if (!infile.ReadLine(largeBuf.data(), largeBuf.size())) goto error;
			char* archivesStr = largeBuf.data();
			if (!strncmp(archivesStr, "ExtractedArchives=", 18))
			{
				archivesStr += 18;
			}
			std::vector<CString>* archives = target->GetExtractedArchives();
			if (archives)
			{
				archives->clear();
				size_t cursor = 0, tokStart = 0, tokLen = 0;
				while (nzbget_rs_dqh_diskstate_next_comma_token(
					archivesStr, cursor, &tokStart, &tokLen, &cursor))
				{
					archives->emplace_back(archivesStr + tokStart, (int)tokLen);
				}
			}
		}
	}
	return true;

error:
	error("Error reading duplicate repair state from disk");
	return false;
}

inline void DiskState::SaveNzbInfo(NzbInfo* nzbInfo, StateDiskFile& outfile)
{
	NzbgetRsDiskNzbInfoInput fields{};
	fields.id = nzbInfo->GetId();
	fields.kind = static_cast<int>(nzbInfo->GetKind());
	fields.url = nzbInfo->GetUrl();
	fields.filename = nzbInfo->GetFilename();
	fields.dest_dir = nzbInfo->GetDestDir();
	fields.final_dir = nzbInfo->GetFinalDir();
	fields.hard_link_path = nzbInfo->GetHardLinkPath().c_str();
	fields.meta_name = nzbInfo->GetMetaName().c_str();
	fields.queued_filename = nzbInfo->GetQueuedFilename();
	fields.name = nzbInfo->GetName();
	fields.category = nzbInfo->GetCategory();
	fields.priority = (int)nzbInfo->GetPriority();
	fields.has_post_info = nzbInfo->GetPostInfo() ? 1 : 0;
	fields.post_stage = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
	fields.delete_paused = (int)nzbInfo->GetDeletePaused();
	fields.many_dupe_files = (int)nzbInfo->GetManyDupeFiles();
	fields.feed_id = nzbInfo->GetFeedId();
	fields.par_status = (int)nzbInfo->GetParStatus();
	fields.unpack_status = (int)nzbInfo->GetUnpackStatus();
	fields.move_status = (int)nzbInfo->GetMoveStatus();
	fields.par_rename_status = (int)nzbInfo->GetParRenameStatus();
	fields.rar_rename_status = (int)nzbInfo->GetRarRenameStatus();
	fields.direct_rename_status = (int)nzbInfo->GetDirectRenameStatus();
	fields.delete_status = (int)nzbInfo->GetDeleteStatus();
	fields.mark_status = (int)nzbInfo->GetMarkStatus();
	fields.url_status = (int)nzbInfo->GetUrlStatus();
	fields.unpack_cleaned_up_disk = (int)nzbInfo->GetUnpackCleanedUpDisk();
	fields.health_paused = (int)nzbInfo->GetHealthPaused();
	fields.add_url_paused = (int)nzbInfo->GetAddUrlPaused();
	fields.file_count = nzbInfo->GetFileCount();
	fields.parked_file_count = nzbInfo->GetParkedFileCount();
	fields.message_count = nzbInfo->GetMessageCount();
	fields.min_time = (int)nzbInfo->GetMinTime();
	fields.max_time = (int)nzbInfo->GetMaxTime();
	fields.par_full = (int)nzbInfo->GetParFull();
	fields.force_par_full = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceParFull() : 0;
	fields.force_repair = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceRepair() : 0;
	fields.extra_par_blocks = nzbInfo->GetExtraParBlocks();
	fields.full_content_hash = nzbInfo->GetFullContentHash();
	fields.filtered_content_hash = nzbInfo->GetFilteredContentHash();
	fields.size = nzbInfo->GetSize();
	fields.success_size = nzbInfo->GetSuccessSize();
	fields.failed_size = nzbInfo->GetFailedSize();
	fields.par_size = nzbInfo->GetParSize();
	fields.par_success_size = nzbInfo->GetParSuccessSize();
	fields.par_failed_size = nzbInfo->GetParFailedSize();
	fields.total_articles = nzbInfo->GetTotalArticles();
	fields.success_articles = nzbInfo->GetSuccessArticles();
	fields.failed_articles = nzbInfo->GetFailedArticles();
	fields.dupe_key = nzbInfo->GetDupeKey();
	fields.dupe_mode = (int)nzbInfo->GetDupeMode();
	fields.dupe_score = nzbInfo->GetDupeScore();
	fields.dupe_hint = (int)nzbInfo->GetDupeHint();
	fields.downloaded_size = nzbInfo->GetDownloadedSize();
	fields.download_sec = nzbInfo->GetDownloadSec();
	fields.post_total_sec = nzbInfo->GetPostTotalSec();
	fields.par_sec = nzbInfo->GetParSec();
	fields.repair_sec = nzbInfo->GetRepairSec();
	fields.unpack_sec = nzbInfo->GetUnpackSec();
	fields.skip_script_processing = static_cast<int>(nzbInfo->GetSkipScriptProcessing());
	fields.skip_disk_write = static_cast<int>(nzbInfo->GetSkipDiskWrite());
	fields.desired_server_id = nzbInfo->GetDesiredServerId();

	std::vector<NzbgetRsDiskCompletedFile> completedFiles;
	completedFiles.reserve(nzbInfo->GetCompletedFiles()->size());
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		completedFiles.push_back(NzbgetRsDiskCompletedFile{
			completedFile.GetId(),
			(int)completedFile.GetStatus(),
			completedFile.GetCrc(),
			(int)completedFile.GetParFile(),
			completedFile.GetHash16k(),
			completedFile.GetParSetId(),
			completedFile.GetFilename(),
			completedFile.GetOrigname()
		});
	}

	std::vector<NzbgetRsDiskParam> params;
	params.reserve(nzbInfo->GetParameters()->size());
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		params.push_back(NzbgetRsDiskParam{ parameter.GetName(), parameter.GetValue() });
	}

	std::vector<NzbgetRsDiskScriptStatus> scripts;
	scripts.reserve(nzbInfo->GetScriptStatuses()->size());
	for (ScriptStatus& scriptStatus : nzbInfo->GetScriptStatuses())
	{
		scripts.push_back(NzbgetRsDiskScriptStatus{ (int)scriptStatus.GetStatus(), scriptStatus.GetName() });
	}

	std::vector<NzbgetRsDiskServerStat> stats;
	stats.reserve(nzbInfo->GetServerStats()->size());
	for (ServerStat& serverStat : nzbInfo->GetServerStats())
	{
		stats.push_back(NzbgetRsDiskServerStat{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	std::vector<NzbgetRsDiskFileStub> fileEntries;
	fileEntries.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (!fileInfo->GetDeleted())
		{
			fileEntries.push_back(NzbgetRsDiskFileStub{
				fileInfo->GetId(),
				(int)fileInfo->GetPaused(),
				(int)fileInfo->GetExtraPriority()
			});
		}
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_nzb_info(
		&fields,
		completedFiles.data(), completedFiles.size(),
		params.data(), params.size(),
		scripts.data(), scripts.size(),
		stats.data(), stats.size(),
		fileEntries.data(), fileEntries.size()));
}

inline bool DiskState::LoadNzbInfo(NzbInfo* nzbInfo, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	char buf[10240];
	int parameterCount = 0;
	int skipScriptProcessing = 0;
	int skipDiskWrite = 0;
	int desiredServerId = 0;
	int numItems = 0;

	int id;
	if (infile.ScanLine("%i", &id) != 1) goto error;
	nzbInfo->SetId(id);

	int kind;
	if (infile.ScanLine("%i", &kind) != 1) goto error;
	nzbInfo->SetKind((NzbInfo::EKind)kind);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetUrl(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetFilename(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetDestDir(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetFinalDir(buf);

	if (formatVersion >= 63) 
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		nzbInfo->SetHardLinkPath(buf);
	}

	// format 65 was written only by pre-release DupeArticleFallback builds,
	// which had the stream-repair state below but not the meta name
	if (nzbget_rs_dqh_diskstate_has_meta_name_field(formatVersion) != 0)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		nzbInfo->SetMetaName(buf);
	}

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetQueuedFilename(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (!Util::EmptyStr(buf))
	{
		nzbInfo->SetName(buf);
	}

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetCategory(buf);

	int priority, postStage, deletePaused, manyDupeFiles, feedId;
	if (formatVersion >= 54)
	{
		if (infile.ScanLine("%i,%i,%i,%i,%i", &priority, &postStage, &deletePaused, &manyDupeFiles, &feedId) != 5) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i,%i,%i", &priority, &postStage, &deletePaused, &manyDupeFiles) != 4) goto error;
		feedId = 0;
	}
	nzbInfo->SetPriority(priority);
	nzbInfo->SetDeletePaused((bool)deletePaused);
	nzbInfo->SetManyDupeFiles((bool)manyDupeFiles);
	if (postStage > 0)
	{
		nzbInfo->EnterPostProcess();
		postStage = nzbget_rs_dsp_normalize_post_stage(postStage, formatVersion, (int)PostInfo::ptFinished);
		nzbInfo->GetPostInfo()->SetStage((PostInfo::EStage)postStage);
	}
	nzbInfo->SetFeedId(feedId);

	int parStatus, unpackStatus, moveStatus, parRenameStatus, rarRenameStatus,
		directRenameStatus, deleteStatus, markStatus, urlStatus;
	if (formatVersion >= 60)
	{
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i,%i,%i", &parStatus,
			&unpackStatus, &moveStatus, &parRenameStatus, &rarRenameStatus, &directRenameStatus,
			&deleteStatus, &markStatus, &urlStatus) != 9) goto error;
	}
	else if (formatVersion >= 58)
	{
		directRenameStatus = 0;
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i,%i", &parStatus,
			&unpackStatus, &moveStatus, &parRenameStatus, &rarRenameStatus, &deleteStatus,
			&markStatus, &urlStatus) != 8) goto error;
	}
	else
	{
		rarRenameStatus = directRenameStatus = 0;
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i", &parStatus, &unpackStatus, &moveStatus,
			&parRenameStatus, &deleteStatus, &markStatus, &urlStatus) != 7) goto error;
	}
	nzbInfo->SetParStatus((NzbInfo::EParStatus)parStatus);
	nzbInfo->SetUnpackStatus((NzbInfo::EPostUnpackStatus)unpackStatus);
	nzbInfo->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
	nzbInfo->SetParRenameStatus((NzbInfo::EPostRenameStatus)parRenameStatus);
	nzbInfo->SetRarRenameStatus((NzbInfo::EPostRenameStatus)rarRenameStatus);
	nzbInfo->SetDirectRenameStatus((NzbInfo::EDirectRenameStatus)directRenameStatus);
	nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)deleteStatus);
	nzbInfo->SetMarkStatus((NzbInfo::EMarkStatus)markStatus);
	if (nzbInfo->GetKind() == NzbInfo::nkNzb || (NzbInfo::EUrlStatus)urlStatus >= NzbInfo::lsFailed)
	{
		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)urlStatus);
	}

	int unpackCleanedUpDisk, healthPaused, addUrlPaused;
	if (infile.ScanLine("%i,%i,%i", &unpackCleanedUpDisk, &healthPaused, &addUrlPaused) != 3) goto error;
	nzbInfo->SetUnpackCleanedUpDisk((bool)unpackCleanedUpDisk);
	nzbInfo->SetHealthPaused((bool)healthPaused);
	nzbInfo->SetAddUrlPaused((bool)addUrlPaused);

	int fileCount, parkedFileCount, messageCount;
	if (formatVersion >= 52)
	{
		if (infile.ScanLine("%i,%i,%i", &fileCount, &parkedFileCount, &messageCount) != 3) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i", &fileCount, &parkedFileCount) != 2) goto error;
		messageCount = 0;
	}
	nzbInfo->SetFileCount(fileCount);
	nzbInfo->SetParkedFileCount(parkedFileCount);
	nzbInfo->SetMessageCount(messageCount);

	int minTime, maxTime;
	if (infile.ScanLine("%i,%i", &minTime, &maxTime) != 2) goto error;
	nzbInfo->SetMinTime((time_t)minTime);
	nzbInfo->SetMaxTime((time_t)maxTime);

	if (formatVersion >= 51)
	{
		int parFull, forceParFull, forceRepair, extraParBlocks = 0;
		if (formatVersion >= 55)
		{
			if (infile.ScanLine("%i,%i,%i,%i", &parFull, &forceParFull, &forceRepair, &extraParBlocks) != 4) goto error;
		}
		else
		{
			if (infile.ScanLine("%i,%i,%i", &parFull, &forceParFull, &forceRepair) != 3) goto error;
		}
		nzbInfo->SetParFull((bool)parFull);
		nzbInfo->SetExtraParBlocks(extraParBlocks);
		if (nzbInfo->GetPostInfo())
		{
			nzbInfo->GetPostInfo()->SetForceParFull((bool)forceParFull);
			nzbInfo->GetPostInfo()->SetForceRepair((bool)forceRepair);
		}
	}

	uint32 fullContentHash, filteredContentHash;
	if (infile.ScanLine("%u,%u", &fullContentHash, &filteredContentHash) != 2) goto error;
	nzbInfo->SetFullContentHash(fullContentHash);
	nzbInfo->SetFilteredContentHash(filteredContentHash);

	uint32 High1, Low1, High2, Low2, High3, Low3;
	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	nzbInfo->SetSize(Util::JoinInt64(High1, Low1));
	nzbInfo->SetSuccessSize(Util::JoinInt64(High2, Low2));
	nzbInfo->SetFailedSize(Util::JoinInt64(High3, Low3));
	nzbInfo->SetCurrentSuccessSize(nzbInfo->GetSuccessSize());
	nzbInfo->SetCurrentFailedSize(nzbInfo->GetFailedSize());

	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	nzbInfo->SetParSize(Util::JoinInt64(High1, Low1));
	nzbInfo->SetParSuccessSize(Util::JoinInt64(High2, Low2));
	nzbInfo->SetParFailedSize(Util::JoinInt64(High3, Low3));
	nzbInfo->SetParCurrentSuccessSize(nzbInfo->GetParSuccessSize());
	nzbInfo->SetParCurrentFailedSize(nzbInfo->GetParFailedSize());

	int totalArticles, successArticles, failedArticles;
	if (infile.ScanLine("%i,%i,%i", &totalArticles, &successArticles, &failedArticles) != 3) goto error;
	nzbInfo->SetTotalArticles(totalArticles);
	nzbInfo->SetSuccessArticles(successArticles);
	nzbInfo->SetFailedArticles(failedArticles);
	nzbInfo->SetCurrentSuccessArticles(successArticles);
	nzbInfo->SetCurrentFailedArticles(failedArticles);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetDupeKey(buf);

	int dupeMode, dupeScore, dupeHint;
	dupeHint = 0; //clang requires initialization in a separate line (due to goto statements)
	if (formatVersion >= 61)
	{
		if (infile.ScanLine("%i,%i,%i", &dupeMode, &dupeScore, &dupeHint) != 3) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i", &dupeMode, &dupeScore) != 2) goto error;
	}
	nzbInfo->SetDupeMode((EDupeMode)dupeMode);
	nzbInfo->SetDupeScore(dupeScore);
	// (the hint went into the mode: every item came back from a restart with its
	// mode replaced by its hint - FORCE and ALL read SCORE)
	nzbInfo->SetDupeHint((NzbInfo::EDupeHint)dupeHint);

	if (formatVersion >= 48)
	{
		uint32 High4, Low4, downloadSec, postTotalSec, parSec, repairSec, unpackSec;
		if (infile.ScanLine("%u,%u,%i,%i,%i,%i,%i", &High4, &Low4, &downloadSec, &postTotalSec, &parSec, &repairSec, &unpackSec) != 7) goto error;
		nzbInfo->SetDownloadedSize(Util::JoinInt64(High4, Low4));
		nzbInfo->SetDownloadSec(downloadSec);
		nzbInfo->SetPostTotalSec(postTotalSec);
		nzbInfo->SetParSec(parSec);
		nzbInfo->SetRepairSec(repairSec);
		nzbInfo->SetUnpackSec(unpackSec);
	}

	nzbInfo->GetCompletedFiles()->clear();
	if (infile.ScanLine("%i", &fileCount) != 1) goto error;
	for (int i = 0; i < fileCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		int id = 0;
		const char* fileName = buf;
		int status = 0;
		uint32 crc = 0;
		int parFile = 0;
		const char* hash16k = nullptr;
		const char* parSetId = nullptr;
		char filenameBuf[1024];
		char origName[1024] = {};

		if (!nzbget_rs_diskstate_parse_completed_file_line(
			buf, formatVersion, &id, &status, &crc, &parFile, &hash16k, &parSetId, &fileName))
		{
			goto error;
		}
		if (formatVersion >= 62)
		{
			if (!infile.ReadLine(filenameBuf, sizeof(filenameBuf))) goto error;
			fileName = filenameBuf;
			if (!infile.ReadLine(origName, sizeof(origName))) goto error;
		}

		nzbInfo->GetCompletedFiles()->emplace_back(
			id, 
			Util::EmptyStr(fileName) ? "" : fileName,
			Util::EmptyStr(origName) ? "" : origName,
			static_cast<CompletedFile::EStatus>(status), 
			crc, 
			static_cast<bool>(parFile),
			Util::EmptyStr(hash16k) ? "" : hash16k,
			Util::EmptyStr(parSetId) ? "" : parSetId);
	}

	nzbInfo->GetParameters()->clear();

	numItems = infile.ScanLine("%i,%i,%i,%i", 
		&parameterCount, 
		&skipScriptProcessing, 
		&skipDiskWrite,
		&desiredServerId);

	{
		if (numItems != 1 && numItems != 4) goto error;
		if (numItems == 4)
		{
			nzbInfo->SetSkipScriptProcessing(static_cast<bool>(skipScriptProcessing));
			nzbInfo->SetSkipDiskWrite(static_cast<bool>(skipDiskWrite));
			nzbInfo->SetDesiredServerId(desiredServerId);
		}
	}

	for (int i = 0; i < parameterCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		size_t eqPos = 0;
		if (nzbget_rs_dqh_diskstate_find_char_pos(buf, '=', &eqPos) != 0)
		{
			buf[eqPos] = '\0';
			nzbInfo->GetParameters()->SetParameter(buf, buf + eqPos + 1);
		}
	}

	nzbInfo->GetScriptStatuses()->clear();
	int scriptCount;
	if (infile.ScanLine("%i", &scriptCount) != 1) goto error;
	for (int i = 0; i < scriptCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		size_t commaPos = 0;
		if (nzbget_rs_dqh_diskstate_find_char_pos(buf, ',', &commaPos) != 0)
		{
			const char* scriptName = buf + commaPos + 1;
			int status = atoi(buf);
			if (status > 1 && formatVersion < 25) status--;
			nzbInfo->GetScriptStatuses()->emplace_back(scriptName, (ScriptStatus::EStatus)status);
		}
	}

	if (!LoadServerStats(nzbInfo->GetServerStats(), servers, infile)) goto error;
	nzbInfo->GetCurrentServerStats()->ListOp(nzbInfo->GetServerStats(), ServerStatList::soSet);

	if (formatVersion < 52)
	{
		int logCount;
		if (infile.ScanLine("%i", &logCount) != 1) goto error;
		for (int i = 0; i < logCount; i++)
		{
			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		}
	}

	nzbInfo->GetFileList()->clear();
	if (infile.ScanLine("%i", &fileCount) != 1) goto error;
	for (int i = 0; i < fileCount; i++)
	{
		uint32 id, paused, time;
		int extraPriority;

		if (formatVersion >= 56)
		{
			if (infile.ScanLine("%i,%i,%i", &id, &paused, &extraPriority) != 3) goto error;
		}
		else
		{
			if (infile.ScanLine("%i,%i,%i,%i", &id, &paused, &time, &extraPriority) != 4) goto error;
		}

		std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>();
		fileInfo->SetId(id);
		fileInfo->SetPaused(paused);
		if (formatVersion < 56)
		{
			fileInfo->SetTime(time);
		}
		fileInfo->SetExtraPriority((bool)extraPriority);
		fileInfo->SetNzbInfo(nzbInfo);
		nzbInfo->GetFileList()->Add(std::move(fileInfo));
	}

	nzbInfo->GetStreamRepairJobs()->clear();
	// earlier builds of this branch wrote it inline (formats 65 and 66); the trailer
	// of the queue file carries it since
	if ((formatVersion == 65 || formatVersion == 66) &&
		!LoadDupeExtras(nzbInfo, infile, formatVersion)) goto error;

	// restore unpack state (R3)
	if (formatVersion >= 67)
	{
		char line2[1024];
		char line3[1024];
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!infile.ReadLine(line2, sizeof(line2))) goto error;
		if (!infile.ReadLine(line3, sizeof(line3))) goto error;
		int directUnpackStatus = 0, unpackTriedBool = 0, parRepairedBool = 0, lastUnpackStatus = 0;
		if (!nzbget_rs_dsp_parse_unpack_state_lines(
			buf, line2, line3,
			static_cast<int>(NzbInfo::nsRunning), static_cast<int>(NzbInfo::nsNone),
			&directUnpackStatus, &unpackTriedBool, &parRepairedBool, &lastUnpackStatus)) goto error;
		nzbInfo->SetDirectUnpackStatus(static_cast<NzbInfo::EDirectUnpackStatus>(directUnpackStatus));
		nzbInfo->SetUnpackTried(unpackTriedBool != 0);
		nzbInfo->SetParRepaired(parRepairedBool != 0);
		nzbInfo->SetLastUnpackStatus(lastUnpackStatus);

		std::vector<char> largeBuf(65536);
		if (!infile.ReadLine(largeBuf.data(), largeBuf.size())) goto error;
		char* archivesStr = largeBuf.data();
		if (!strncmp(archivesStr, "ExtractedArchives=", 18))
		{
			archivesStr += 18;
		}
		std::vector<CString>* archives = nzbInfo->GetExtractedArchives();
		if (archives)
		{
			archives->clear();
			size_t cursor = 0, tokStart = 0, tokLen = 0;
			while (nzbget_rs_dqh_diskstate_next_comma_token(
				archivesStr, cursor, &tokStart, &tokLen, &cursor))
			{
				archives->emplace_back(archivesStr + tokStart, (int)tokLen);
			}
		}
	}

	return true;

error:
	error("Error reading nzb info from disk");
	return false;
}

inline bool DiskState::LoadServerStats(ServerStatList* serverStatList, Servers* servers, StateDiskFile& infile)
{
	int statCount;
	if (infile.ScanLine("%i", &statCount) != 1) goto error;
	for (int i = 0; i < statCount; i++)
	{
		int serverId, successArticles, failedArticles;
		if (infile.ScanLine("%i,%i,%i", &serverId, &successArticles, &failedArticles) != 3) goto error;

		if (servers)
		{
			// find server (id could change if config file was edited)
			for (NewsServer* newsServer : servers)
			{
				if (nzbget_rs_dqh_diskstate_matches_server_state_id(
					newsServer->GetStateId(), serverId) != 0)
				{
					serverStatList->StatOp(newsServer->GetId(), successArticles, failedArticles, ServerStatList::soSet);
				}
			}
		}
	}

	return true;

error:
	error("Error reading server stats from disk");
	return false;
}

inline bool DiskState::SaveFile(FileInfo* fileInfo, bool replace)
{
	debug("Saving FileInfo %i to disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_conn_format_port_str(fileInfo->GetId());
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, replace);
	stateFile.SetFlush(!replace);

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	return SaveFileInfo(fileInfo, *outfile, true) && stateFile.FinishWrite();
}

inline bool DiskState::SaveFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, bool articles)
{
	NzbgetRsDiskFileInfoInput fields{};
	fields.subject = fileInfo->GetSubject();
	fields.filename = fileInfo->GetFilename();
	fields.origname = fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "";
	fields.output_filename = fileInfo->GetOutputFilename().c_str();
	fields.filename_confirmed = (int)fileInfo->GetFilenameConfirmed();
	fields.time = (int)fileInfo->GetTime();
	fields.output_initialized = (int)fileInfo->GetOutputInitialized();
	fields.size = fileInfo->GetSize();
	fields.missed_size = fileInfo->GetMissedSize();
	fields.par_file = (int)fileInfo->GetParFile();
	fields.total_articles = fileInfo->GetTotalArticles();
	fields.missed_articles = fileInfo->GetMissedArticles();

	std::vector<const char*> groups;
	groups.reserve(fileInfo->GetGroups()->size());
	for (CString& group : fileInfo->GetGroups())
	{
		groups.push_back(*group);
	}

	std::vector<NzbgetRsDiskArticleInfo> articleEntries;
	if (articles)
	{
		articleEntries.reserve(fileInfo->GetArticles()->size());
		for (ArticleInfo* articleInfo : fileInfo->GetArticles())
		{
			articleEntries.push_back(NzbgetRsDiskArticleInfo{
				articleInfo->GetPartNumber(),
				articleInfo->GetSize(),
				nzbget_rs_dqh_diskstate_select_original_or_current_msg_id(
					articleInfo->GetDupeOriginalMessageId(),
					articleInfo->GetMessageId())
			});
		}
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_file_info(
		&fields,
		groups.data(), groups.size(),
		articles ? 1 : 0,
		articleEntries.data(), articleEntries.size()));

	return true;
}

inline bool DiskState::LoadArticles(FileInfo* fileInfo)
{
	return LoadFile(fileInfo, false, true);
}

inline bool DiskState::LoadFile(FileInfo* fileInfo, bool fileSummary, bool articles)
{
	debug("Loading FileInfo %i from disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_conn_format_port_str(fileInfo->GetId());
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	return LoadFileInfo(fileInfo, *infile, stateFile.GetFileVersion(), fileSummary, articles);
}

inline bool DiskState::LoadFileInfo(FileInfo* fileInfo, StateDiskFile& infile, int formatVersion, bool fileSummary, bool articles)
{
	char buf[1024];

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetSubject(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetFilename(buf);

	if (formatVersion >= 6)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetOrigname(buf);
	}

	if (formatVersion >= 7)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!Util::EmptyStr(buf))
		{
			fileInfo->SetOutputFilename(buf);
		}
	}

	if (formatVersion >= 5)
	{
		int time = 0, filenameConfirmed = 0, outputInitialized = 0;
		int cnt = infile.ScanLine("%i,%i,%i", &filenameConfirmed, &time, &outputInitialized);
		if (cnt < 2) goto error;
		if (fileSummary) fileInfo->SetFilenameConfirmed(static_cast<bool>(filenameConfirmed));
		if (fileSummary) fileInfo->SetTime(static_cast<time_t>(time));
		if (fileSummary && cnt >= 3)
		{
			bool directWriteOpt = g_Options && g_Options->GetDirectWrite();
			bool forceDirectWrite = fileInfo->GetForceDirectWrite();
			bool hasOwnOutFile = false;
			bool hasOutTmp = false;
			bool hasForceDest = false;
			if (nzbget_rs_dqh_diskstate_should_probe_direct_write_files(
				outputInitialized, directWriteOpt ? 1 : 0, forceDirectWrite ? 1 : 0) != 0)
			{
				const char* destDir = fileInfo->GetNzbInfo() ? fileInfo->GetNzbInfo()->GetDestDir() : nullptr;
				hasOwnOutFile = !fileInfo->GetOutputFilename().empty() &&
					FileSystem::FileExists(fileInfo->GetOutputFilename().c_str());
				if (destDir && !Util::EmptyStr(destDir))
				{
					NzbgetRsBuf outTmpBuf = nzbget_rs_dqh_diskstate_format_out_tmp_path(
						destDir, PATH_SEPARATOR, fileInfo->GetId());
					hasOutTmp = FileSystem::FileExists(outTmpBuf.data ? outTmpBuf.data : "");
					nzbget_rs_free(outTmpBuf);
				}
				if (forceDirectWrite && destDir && !Util::EmptyStr(destDir) && !Util::EmptyStr(fileInfo->GetFilename()))
				{
					NzbgetRsBuf forceDestBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
						destDir, PATH_SEPARATOR, fileInfo->GetFilename());
					hasForceDest = FileSystem::FileExists(forceDestBuf.data ? forceDestBuf.data : "");
					nzbget_rs_free(forceDestBuf);
				}
			}
			bool restoredOutputInit = nzbget_rs_dsp_eval_restored_output_init(
				outputInitialized,
				directWriteOpt ? 1 : 0,
				forceDirectWrite ? 1 : 0,
				hasOwnOutFile ? 1 : 0,
				hasOutTmp ? 1 : 0,
				hasForceDest ? 1 : 0) != 0;
			fileInfo->SetOutputInitialized(restoredOutputInit);
		}
	}
	else if (formatVersion >= 4)
	{
		int time;
		if (infile.ScanLine("%i", &time) != 1) goto error;
		if (fileSummary) fileInfo->SetTime(static_cast<time_t>(time));
	}

	uint32 High, Low;
	if (infile.ScanLine("%u,%u", &High, &Low) != 2) goto error;
	if (fileSummary) fileInfo->SetSize(Util::JoinInt64(High, Low));
	if (fileSummary) fileInfo->SetRemainingSize(fileInfo->GetSize());

	if (infile.ScanLine("%u,%u", &High, &Low) != 2) goto error;
	if (fileSummary) fileInfo->SetMissedSize(Util::JoinInt64(High, Low));
	if (fileSummary) fileInfo->SetRemainingSize(fileInfo->GetSize() - fileInfo->GetMissedSize());

	int parFile;
	if (infile.ScanLine("%i", &parFile) != 1) goto error;
	if (fileSummary) fileInfo->SetParFile(static_cast<bool>(parFile));

	int totalArticles, missedArticles;
	if (infile.ScanLine("%i,%i", &totalArticles, &missedArticles) != 2) goto error;
	if (fileSummary) fileInfo->SetTotalArticles(totalArticles);
	if (fileSummary) fileInfo->SetMissedArticles(missedArticles);

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (fileSummary && !Util::EmptyStr(buf)) fileInfo->GetGroups()->push_back(buf);
	}

	if (articles)
	{
		if (infile.ScanLine("%i", &size) != 1) goto error;
		for (int i = 0; i < size; i++)
		{
			int PartNumber, PartSize;
			if (infile.ScanLine("%i,%i", &PartNumber, &PartSize) != 2) goto error;

			if (!infile.ReadLine(buf, sizeof(buf))) goto error;

			std::unique_ptr<ArticleInfo> articleInfo = std::make_unique<ArticleInfo>();
			articleInfo->SetPartNumber(PartNumber);
			articleInfo->SetSize(PartSize);
			articleInfo->SetMessageId(buf);
			fileInfo->GetArticles()->push_back(std::move(articleInfo));
		}
	}

	return true;

error:
	error("Error reading diskstate for file %i", fileInfo->GetId());
	return false;
}

inline bool DiskState::SaveFileState(FileInfo* fileInfo, bool completed, bool replace)
{
	debug("Saving FileState %i to disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_diskstate_format_file_state_name(
		fileInfo->GetId(), completed ? 1 : 0);
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, replace);
	stateFile.SetFlush(!replace);

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	return SaveFileState(fileInfo, *outfile, completed) && stateFile.FinishWrite();
}

inline bool DiskState::SaveFileState(FileInfo* fileInfo, StateDiskFile& outfile, bool completed)
{
	NzbgetRsDiskFileStateInput fields{};
	fields.success_articles = fileInfo->GetSuccessArticles();
	fields.failed_articles = fileInfo->GetFailedArticles();
	fields.remaining_size = fileInfo->GetRemainingSize();
	fields.success_size = fileInfo->GetSuccessSize();
	fields.failed_size = fileInfo->GetFailedSize();
	fields.filename = fileInfo->GetFilename();
	fields.hash16k = fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "";
	fields.par_set_id = fileInfo->GetParSetId() ? fileInfo->GetParSetId() : "";
	fields.par_file = (int)fileInfo->GetParFile();
	fields.decoded_file_size = fileInfo->GetDecodedFileSize();
	fields.output_initialized = (int)fileInfo->GetOutputInitialized();

	std::vector<NzbgetRsDiskServerStat> stats;
	stats.reserve(fileInfo->GetServerStats()->size());
	for (ServerStat& serverStat : fileInfo->GetServerStats())
	{
		stats.push_back(NzbgetRsDiskServerStat{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	std::vector<NzbgetRsDiskArticleState> articles;
	articles.reserve(fileInfo->GetArticles()->size());
	for (ArticleInfo* articleInfo : fileInfo->GetArticles())
	{
		int savedStatus = 0;
		int stagedFallback = nzbget_rs_dsp_calc_staged_fallback(
			completed ? 1 : 0,
			(int)articleInfo->GetStatus(),
			(int)ArticleInfo::aiFinished,
			articleInfo->GetDupeFallbackRound(),
			articleInfo->GetResultFilename() ? 1 : 0,
			(int)ArticleInfo::aiUndefined,
			&savedStatus);
		articles.push_back(NzbgetRsDiskArticleState{
			savedStatus,
			articleInfo->GetSegmentOffset(),
			articleInfo->GetSegmentSize(),
			articleInfo->GetCrc(),
			stagedFallback
		});
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_file_state(
		&fields,
		stats.data(), stats.size(),
		articles.data(), articles.size()));

	outfile.Close();
	return true;
}

inline bool DiskState::LoadFileState(FileInfo* fileInfo, Servers* servers, bool completed)
{
	debug("Loading FileInfo %i from disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_diskstate_format_file_state_name(
		fileInfo->GetId(), completed ? 1 : 0);
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	m_lastFileStateVersion = stateFile.GetFileVersion();
	return LoadFileState(fileInfo, servers, *infile, stateFile.GetFileVersion(), completed);
}

inline bool DiskState::LoadFileState(FileInfo* fileInfo, Servers* servers, StateDiskFile& infile, int formatVersion, bool completed)
{
	bool hasArticles = !fileInfo->GetArticles()->empty();

	int successArticles, failedArticles;
	if (infile.ScanLine("%i,%i", &successArticles, &failedArticles) != 2) goto error;
	fileInfo->SetSuccessArticles(successArticles);
	fileInfo->SetFailedArticles(failedArticles);

	uint32 High1, Low1, High2, Low2, High3, Low3;
	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	fileInfo->SetRemainingSize(Util::JoinInt64(High1, Low1));
	fileInfo->SetSuccessSize(Util::JoinInt64(High2, Low2));
	fileInfo->SetFailedSize(Util::JoinInt64(High3, Low3));
	if (formatVersion >= 8)
	{
		uint32 decodedHigh, decodedLow;
		if (infile.ScanLine("%u,%u", &decodedHigh, &decodedLow) != 2) goto error;
		fileInfo->SetDecodedFileSize(Util::JoinInt64(decodedHigh, decodedLow));
	}

	char buf[1024];

	if (formatVersion >= 4)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!Util::EmptyStr(buf)) fileInfo->SetFilename(buf);
	}

	if (formatVersion >= 5)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		fileInfo->SetHash16k(*buf ? buf : nullptr);
		if (formatVersion >= 6)
		{
			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
			fileInfo->SetParSetId(*buf ? buf : nullptr);
		}
		int parFile = 0;
		if (infile.ScanLine("%i", &parFile) != 1) goto error;
		fileInfo->SetParFile((bool)parFile);
	}

	if (!LoadServerStats(fileInfo->GetServerStats(), servers, infile)) goto error;

	int completedArticles;
	completedArticles = 0; //clang requires initialization in a separate line (due to goto statements)

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	// the state of a file with its articles loaded can't list more (a stale or
	// damaged state file did: at() threw out of the queue coordinator). Fewer is
	// fine, as upstream has it: older queues saved some states with none
	if (!nzbget_rs_dsp_validate_article_count(
		size, hasArticles ? 1 : 0, (int)fileInfo->GetArticles()->size()))
	{
		goto error;
	}
	for (int i = 0; i < size; i++)
	{
		if (!hasArticles)
		{
			fileInfo->GetArticles()->push_back(std::make_unique<ArticleInfo>());
		}
		std::unique_ptr<ArticleInfo>& pa = fileInfo->GetArticles()->at(i);

		int statusInt = 0;

		int64 segmentOffset = 0;
		uint32 crc = 0;
		int segmentSize = 0;
		int stagedFallback = 0;
		int fields = 0;
		if (formatVersion >= 2)
		{
			fields = infile.ScanLine("%i,%" PRIi64 ",%i,%u,%i", &statusInt, &segmentOffset,
				&segmentSize, &crc, &stagedFallback);
		}
		else
		{
			fields = infile.ScanLine("%i", &statusInt);
		}
		bool needStagedCheck = nzbget_rs_dsp_need_staged_file_check(
			completed ? 1 : 0,
			formatVersion,
			statusInt,
			(int)ArticleInfo::aiFinished,
			segmentOffset,
			segmentSize,
			stagedFallback,
			pa->GetPartNumber(),
			g_Options->GetDirectWrite() ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0) != 0;
		bool stagedFileExists = false;
		if (needStagedCheck)
		{
			NzbgetRsBuf stagedBuf = nzbget_rs_dqh_diskstate_format_staged_part_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, fileInfo->GetId(), pa->GetPartNumber());
			stagedFileExists = FileSystem::FileExists(stagedBuf.data ? stagedBuf.data : "");
			nzbget_rs_free(stagedBuf);
		}
		NzbgetRsArticleLoadEval eval{};
		if (!nzbget_rs_dsp_eval_article_load_state(
			formatVersion,
			completed ? 1 : 0,
			fields,
			statusInt,
			segmentOffset,
			segmentSize,
			stagedFallback,
			pa->GetPartNumber(),
			stagedFileExists ? 1 : 0,
			g_Options->GetDirectWrite() ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0,
			pa->GetResultFilename() ? 1 : 0,
			completedArticles,
			size,
			&eval))
		{
			goto error;
		}
		if (formatVersion >= 2)
		{
			pa->SetSegmentOffset(eval.segment_offset);
			pa->SetSegmentSize(eval.segment_size);
			pa->SetCrc(crc);
		}
		pa->SetDupeFallbackRound(eval.dupe_fallback_round);
		if (eval.need_result_filename)
		{
			NzbgetRsBuf resBuf = nzbget_rs_dqh_diskstate_format_staged_part_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, fileInfo->GetId(), pa->GetPartNumber());
			pa->SetResultFilename(resBuf.data ? resBuf.data : "");
			nzbget_rs_free(resBuf);
		}
		completedArticles = eval.completed_articles;
		pa->SetStatus((ArticleInfo::EStatus)eval.status);
	}

	fileInfo->SetCompletedArticles(completedArticles);

	if (formatVersion < 8)
	{
		// data after upstream's data, where this build wrote it
		char trailer[1024];
		while (infile.ReadLine(trailer, sizeof(trailer)))
		{
			int64 decodedSize = 0;
			int outputInit = 0;
			int trailerKind = nzbget_rs_dsp_parse_legacy_file_state_trailer(
				trailer, &decodedSize, &outputInit);
			if (trailerKind == 1)
			{
				fileInfo->SetDecodedFileSize(decodedSize);
			}
			else if (trailerKind == 2)
			{
				fileInfo->SetOutputInitialized(outputInit != 0);
			}
		}
	}

	// Validate or recover DirectWrite OutputInitialized against the actual on-disk direct-write output file
	if (g_Options && (g_Options->GetDirectWrite() || fileInfo->GetForceDirectWrite()))
	{
		const char* destDir = fileInfo->GetNzbInfo() ? fileInfo->GetNzbInfo()->GetDestDir() : nullptr;
		bool hasOwnOutFile = !fileInfo->GetOutputFilename().empty() &&
			FileSystem::FileExists(fileInfo->GetOutputFilename().c_str());
		bool hasOutTmp = false;
		if (destDir && !Util::EmptyStr(destDir))
		{
			NzbgetRsBuf outTmpBuf = nzbget_rs_dqh_diskstate_format_out_tmp_path(
				destDir, PATH_SEPARATOR, fileInfo->GetId());
			hasOutTmp = FileSystem::FileExists(outTmpBuf.data ? outTmpBuf.data : "");
			nzbget_rs_free(outTmpBuf);
		}
		bool hasCompletedDest = false;
		if (nzbget_rs_dqh_diskstate_can_check_completed_dest_file(
			completed ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0,
			Util::EmptyStr(destDir) ? 1 : 0,
			Util::EmptyStr(fileInfo->GetFilename()) ? 1 : 0) != 0)
		{
			NzbgetRsBuf compDestBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				destDir, PATH_SEPARATOR, fileInfo->GetFilename());
			hasCompletedDest = FileSystem::FileExists(compDestBuf.data ? compDestBuf.data : "");
			nzbget_rs_free(compDestBuf);
		}
		bool hasDirectWriteOutput = hasOwnOutFile || hasOutTmp || hasCompletedDest;

		int newOutputInitialized = 0;
		int pickPathKind = 0;
		nzbget_rs_dsp_reconcile_direct_write_output(
			fileInfo->GetOutputInitialized() ? 1 : 0,
			hasOwnOutFile ? 1 : 0,
			hasOutTmp ? 1 : 0,
			hasCompletedDest ? 1 : 0,
			fileInfo->GetSuccessArticles(),
			completed ? 1 : 0,
			fileInfo->GetOutputFilename().empty() ? 1 : 0,
			&newOutputInitialized,
			&pickPathKind);
		fileInfo->SetOutputInitialized(newOutputInitialized != 0);
		if (pickPathKind != 0)
		{
			NzbgetRsBuf pickedBuf = pickPathKind == 1 ?
				nzbget_rs_dqh_diskstate_format_out_tmp_path(destDir, PATH_SEPARATOR, fileInfo->GetId()) :
				nzbget_rs_tpr_article_writer_format_dest_file_path(destDir, PATH_SEPARATOR, fileInfo->GetFilename());
			fileInfo->SetOutputFilename(pickedBuf.data ? pickedBuf.data : "");
			nzbget_rs_free(pickedBuf);
		}
	}

	infile.Close();
	return true;

error:
	infile.Close();
	error("Error reading diskstate for file %i", fileInfo->GetId());
	return false;
}

inline void DiskState::DiscardFiles(NzbInfo* nzbInfo, bool deleteLog)
{
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		DiscardFile(fileInfo->GetId(), true, true, true);
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetStatus() != CompletedFile::cfSuccess)
		{
			DiscardFile(completedFile.GetId(), true, true, true);
		}
	}

	if (deleteLog)
	{
		NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, nzbInfo->GetId());
		FileSystem::DeleteFile(logBuf.data ? logBuf.data : "");
		nzbget_rs_free(logBuf);
	}
}

inline void DiskState::SaveDupInfo(DupInfo* dupInfo, StateDiskFile& outfile)
{
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_dup_info(
		(int)dupInfo->GetStatus(),
		dupInfo->GetSize(),
		dupInfo->GetFullContentHash(),
		dupInfo->GetFilteredContentHash(),
		dupInfo->GetDupeScore(),
		(int)dupInfo->GetDupeMode(),
		dupInfo->GetName(),
		dupInfo->GetDupeKey()));
}

inline bool DiskState::LoadDupInfo(DupInfo* dupInfo, StateDiskFile& infile, int formatVersion)
{
	char buf[1024];

	int status;
	uint32 High, Low;
	uint32 fullContentHash, filteredContentHash = 0;
	int dupeScore, dupeMode;
	if (infile.ScanLine("%i,%u,%u,%u,%u,%i,%i", &status, &High, &Low, &fullContentHash, &filteredContentHash, &dupeScore, &dupeMode) != 7) goto error;

	dupInfo->SetStatus((DupInfo::EStatus)status);
	dupInfo->SetFullContentHash(fullContentHash);
	dupInfo->SetFilteredContentHash(filteredContentHash);
	dupInfo->SetSize(Util::JoinInt64(High, Low));
	dupInfo->SetDupeScore(dupeScore);
	dupInfo->SetDupeMode((EDupeMode)dupeMode);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	dupInfo->SetName(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	dupInfo->SetDupeKey(buf);

	return true;

error:
	return false;
}

inline void DiskState::SaveHistory(HistoryList* history, StateDiskFile& outfile)
{
	debug("Saving history to disk");

	outfile.PrintLine("%i", (int)history->size());
	for (HistoryInfo* historyInfo : history)
	{
		outfile.PrintLine("%i,%i,%i", historyInfo->GetId(), (int)historyInfo->GetKind(), (int)historyInfo->GetTime());

		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			SaveNzbInfo(historyInfo->GetNzbInfo(), outfile);
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			SaveDupInfo(historyInfo->GetDupInfo(), outfile);
		}
	}

	std::vector<NzbInfo*> nzbs;
	for (HistoryInfo* historyInfo : history)
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			nzbs.push_back(historyInfo->GetNzbInfo());
		}
	}
	SaveDupeTrailer(nzbs, outfile);
}

inline bool DiskState::LoadHistory(HistoryList* history, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading history from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		std::unique_ptr<HistoryInfo> historyInfo;
		HistoryInfo::EKind kind = HistoryInfo::hkNzb;
		int id = 0;
		int time;

		int kindval = 0;
		if (infile.ScanLine("%i,%i,%i", &id, &kindval, &time) != 3) goto error;
		kind = (HistoryInfo::EKind)kindval;

		if (kind == HistoryInfo::hkNzb)
		{
			std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
			if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
			nzbInfo->LeavePostProcess();
			historyInfo = std::make_unique<HistoryInfo>(std::move(nzbInfo));
		}
		else if (kind == HistoryInfo::hkUrl)
		{
			std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
			if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
			historyInfo = std::make_unique<HistoryInfo>(std::move(nzbInfo));
		}
		else if (kind == HistoryInfo::hkDup)
		{
			std::unique_ptr<DupInfo> dupInfo = std::make_unique<DupInfo>();
			if (!LoadDupInfo(dupInfo.get(), infile, formatVersion)) goto error;
			dupInfo->SetId(id);
			historyInfo = std::make_unique<HistoryInfo>(std::move(dupInfo));
		}
		else
		{
			// a kind no version writes (a damaged record): the file can't be read on
			error("Unknown history record kind %i", kindval);
			goto error;
		}

		historyInfo->SetTime((time_t)time);

		history->push_back(std::move(historyInfo));
	}

	if (formatVersion >= 57 && formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([history](int id) -> NzbInfo*
			{
				for (HistoryInfo* historyInfo : history)
				{
					if (nzbget_rs_dqh_diskstate_matches_history_nzb_id(
						(int)historyInfo->GetKind(),
						(int)HistoryInfo::hkNzb,
						(int)HistoryInfo::hkUrl,
						(historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl) ?
							historyInfo->GetNzbInfo()->GetId() : 0,
						id) != 0)
					{
						return historyInfo->GetNzbInfo();
					}
				}
				return nullptr;
			}, infile)) goto error;

	return true;

error:
	error("Error reading diskstate for history");
	return false;
}

/*
 * Deletes whole download queue including history.
 */
inline void DiskState::DiscardDownloadQueue()
{
	debug("Discarding queue");

	for (const char* name : { "queue", "history" })
	{
		NzbgetRsBuf qBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		FileSystem::DeleteFile(qBuf.data ? qBuf.data : "");
		nzbget_rs_free(qBuf);
	}

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		// delete all files whose names have only characters '0'..'9'
		bool onlyNums = true;
		for (const char* p = filename; *p; p++)
		{
			if (*p < '0' || *p > '9')
			{
				onlyNums = false;
				break;
			}
		}
		if (onlyNums)
		{
			for (char suffix : { '\0', 's', 'c' })
			{
				NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_file_state_path(
					g_Options->GetQueueDir(), PATH_SEPARATOR, filename, suffix);
				FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
				nzbget_rs_free(fBuf);
			}
		}
	}
}

inline bool DiskState::DownloadQueueExists()
{
	debug("Checking if a saved queue exists on disk");

	// An empty queue still has saved state: the IDs handed out before its entries
	// were deleted. Load it so both generators continue past those IDs.
	for (const char* name : { "queue", "history", "ids" })
	{
		NzbgetRsBuf qBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		bool exists = FileSystem::FileExists(qBuf.data ? qBuf.data : "");
		nzbget_rs_free(qBuf);
		if (exists)
		{
			return true;
		}
	}
	return false;
}

inline void DiskState::DiscardFile(int fileId, bool deleteData, bool deletePartialState, bool deleteCompletedState)
{
	// info and articles file
	if (deleteData)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, '\0');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}

	// partial state file
	if (deletePartialState)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, 's');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}

	// completed state file
	if (deleteCompletedState)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, 'c');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}
}

inline void DiskState::CleanupTempDir(DownloadQueue* downloadQueue)
{
	DirBrowser dir(g_Options->GetTempDir());
	while (const char* filename = dir.Next())
	{
		int id = 0, part = 0;
		int kind = nzbget_rs_dsp_classify_temp_file(filename, &id, &part);
		bool garbage = kind == 1;
		if (kind == 2)
		{
			garbage = true;
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				if (nzbInfo->GetFileList()->Find(id))
				{
					garbage = false;
					break;
				}
			}
		}

		if (garbage)
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, filename);
			FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
			nzbget_rs_free(fullFnBuf);
		}
	}
}

inline void DiskState::CleanupQueueDir(DownloadQueue* downloadQueue)
{
	// Prepare sorted id lists for faster search

	std::vector<int> nzbIdList;
	std::vector<int> fileIdList;

	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		nzbIdList.push_back(nzbInfo->GetId());

		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			fileIdList.push_back(fileInfo->GetId());
		}

		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			fileIdList.push_back(completedFile.GetId());
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb ||
			historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
			nzbIdList.push_back(nzbInfo->GetId());

			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				fileIdList.push_back(fileInfo->GetId());
			}

			for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
			{
				fileIdList.push_back(completedFile.GetId());
			}
		}
	}

	std::sort(nzbIdList.begin(), nzbIdList.end());
	std::sort(fileIdList.begin(), fileIdList.end());

	// Do cleanup

	int deletedFiles = 0;

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		bool del = nzbget_rs_dsp_is_orphan_queue_file(
			filename,
			fileIdList.data(), fileIdList.size(),
			nzbIdList.data(), nzbIdList.size()) != 0;

		if (del)
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
			detail("Deleting orphaned diskstate file %s", filename);
			FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
			nzbget_rs_free(fullFnBuf);
			deletedFiles++;
		}
	}

	if (deletedFiles > 0)
	{
		info("Deleted %i orphaned diskstate file(s)", deletedFiles);
	}
}

/* For safety:
 * - first save to temp-file (feeds.new)
 * - then delete feeds
 * - then rename feeds.new to feeds
 */
inline bool DiskState::SaveFeeds(Feeds* feeds, FeedHistory* feedHistory)
{
	debug("Saving feeds state to disk");

	StateFile stateFile("feeds", DISKSTATE_FEEDS_VERSION, true);

	if (nzbget_rs_dqh_diskstate_should_discard_feeds_file(
		feeds->empty() ? 1 : 0, feedHistory->empty() ? 1 : 0) != 0)
	{
		stateFile.Discard();
		return true;
	}

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	// save status
	SaveFeedStatus(feeds, *outfile);

	// save history
	SaveFeedHistory(feedHistory, *outfile);

	// now rename to dest file name
	return stateFile.FinishWrite();
}

inline bool DiskState::LoadFeeds(Feeds* feeds, FeedHistory* feedHistory)
{
	debug("Loading feeds state from disk");

	StateFile stateFile("feeds", DISKSTATE_FEEDS_VERSION, true);

	if (!stateFile.FileExists())
	{
		return true;
	}

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	bool ok = false;
	int formatVersion = stateFile.GetFileVersion();

	// load feed status
	if (!LoadFeedStatus(feeds, *infile, formatVersion)) goto error;

	// load feed history
	if (!LoadFeedHistory(feedHistory, *infile, formatVersion)) goto error;

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for feeds");
	}

	return ok;
}

inline bool DiskState::SaveFeedStatus(Feeds* feeds, StateDiskFile& outfile)
{
	debug("Saving feed status to disk");

	std::vector<const char*> urls;
	std::vector<uint32_t> filterHashes;
	std::vector<int> lastUpdates;
	urls.reserve(feeds->size());
	filterHashes.reserve(feeds->size());
	lastUpdates.reserve(feeds->size());
	for (FeedInfo* feedInfo : feeds)
	{
		urls.push_back(feedInfo->GetUrl());
		filterHashes.push_back(feedInfo->GetFilterHash());
		lastUpdates.push_back((int)feedInfo->GetLastUpdate());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_feed_status(
		urls.data(), filterHashes.data(), lastUpdates.data(), urls.size()));

	return true;
}

inline bool DiskState::LoadFeedStatus(Feeds* feeds, StateDiskFile& infile, int formatVersion)
{
	debug("Loading feed status from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		char url[1024];
		if (!infile.ReadLine(url, sizeof(url))) goto error;

		char filter[1024];
		if (formatVersion == 2)
		{
			if (!infile.ReadLine(filter, sizeof(filter))) goto error;
		}

		uint32 filterHash = 0;
		if (formatVersion >= 3)
		{
			if (infile.ScanLine("%u", &filterHash) != 1) goto error;
		}

		int lastUpdate = 0;
		if (infile.ScanLine("%i", &lastUpdate) != 1) goto error;

		for (FeedInfo* feedInfo : feeds)
		{
			if (nzbget_rs_dsp_feed_status_matches(
				feedInfo->GetUrl(),
				url,
				formatVersion,
				formatVersion == 2 ? feedInfo->GetFilter() : "",
				formatVersion == 2 ? filter : "",
				feedInfo->GetFilterHash(),
				filterHash) != 0)
			{
				feedInfo->SetLastUpdate((time_t)lastUpdate);
			}
		}
	}

	return true;

error:
	error("Error reading feed status from disk");
	return false;
}

inline bool DiskState::SaveFeedHistory(FeedHistory* feedHistory, StateDiskFile& outfile)
{
	debug("Saving feed history to disk");

	std::vector<int> statuses;
	std::vector<int> lastSeens;
	std::vector<const char*> urls;
	statuses.reserve(feedHistory->size());
	lastSeens.reserve(feedHistory->size());
	urls.reserve(feedHistory->size());
	for (FeedHistoryInfo& feedHistoryInfo : feedHistory)
	{
		statuses.push_back((int)feedHistoryInfo.GetStatus());
		lastSeens.push_back((int)feedHistoryInfo.GetLastSeen());
		urls.push_back(feedHistoryInfo.GetUrl());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_feed_history(
		statuses.data(), lastSeens.data(), urls.data(), urls.size()));

	return true;
}

inline bool DiskState::LoadFeedHistory(FeedHistory* feedHistory, StateDiskFile& infile, int formatVersion)
{
	debug("Loading feed history from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		int status = 0;
		int lastSeen = 0;
		int r = infile.ScanLine("%i,%i", &status, &lastSeen);
		if (r != 2) goto error;

		char url[1024];
		if (!infile.ReadLine(url, sizeof(url))) goto error;

		feedHistory->emplace_back(url, (FeedHistoryInfo::EStatus)(status), (time_t)(lastSeen));
	}

	return true;

error:
	error("Error reading feed history from disk");
	return false;
}

inline void DiskState::CalcFileStats(DownloadQueue* downloadQueue, int formatVersion)
{
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		nzbInfo->UpdateCurrentStats();
	}
}

inline bool DiskState::SaveAllFileInfos(DownloadQueue* downloadQueue)
{
	bool ok = true;
	StateFile stateFile("files", DISKSTATE_FILE_VERSION, true);
	if (!downloadQueue->GetQueue()->empty())
	{
		StateDiskFile* outfile = stateFile.BeginWrite();
		if (!outfile)
		{
			return false;
		}

		// save file-infos

		int fileCount = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			fileCount += nzbInfo->GetFileList()->size();
		}
		outfile->PrintLine("%i", fileCount);

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				outfile->PrintLine("%i", fileInfo->GetId());
				SaveFileInfo(fileInfo, *outfile, false);
			}
		}

		// now rename to dest file name
		ok = stateFile.FinishWrite();
	}
	else
	{
		stateFile.Discard();
	}

	return ok;
}

inline bool DiskState::LoadAllFileInfos(DownloadQueue* downloadQueue)
{
	if (downloadQueue->GetQueue()->empty())
	{
		return true;
	}

	StateFile stateFile("files", DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);
	StateDiskFile* infile = nullptr;
	bool useHibernate = false;

	if (stateFile.FileExists())
	{
		infile = stateFile.BeginRead();
		useHibernate = infile != nullptr;
		if (useHibernate)
		{
			int fileCount = 0;
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				fileCount += nzbInfo->GetFileList()->size();
			}
			int size = 0;
			int scanCnt = infile->ScanLine("%i", &size);
			useHibernate = scanCnt == 1 && size == fileCount;
		}
		if (!useHibernate)
		{
			stateFile.Discard();
		}
	}

	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		RawFileList brokenFileInfos;

		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			bool res = false;
			if (useHibernate)
			{
				int id = 0;
				infile->ScanLine("%i", &id);
				if (id == fileInfo->GetId())
				{
					res = LoadFileInfo(fileInfo, *infile, stateFile.GetFileVersion(), true, false);
				}
			}
			if (!res)
			{
				res = LoadFile(fileInfo, true, false);
			}
			if (!res)
			{
				brokenFileInfos.push_back(fileInfo);
			}
		}

		for (FileInfo* fileInfo : brokenFileInfos)
		{
			nzbInfo->GetFileList()->Remove(fileInfo);
		}
	}

	return true;
}

inline void DiskState::DiscardQuickFileInfos()
{
	StateFile stateFile("files", DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);
	stateFile.Discard();
}

inline bool DiskState::LoadAllFileStates(DownloadQueue* downloadQueue, Servers* servers)
{
	NzbgetRsBuf cacheFlagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	bool cacheWasActive = FileSystem::FileExists(cacheFlagBuf.data ? cacheFlagBuf.data : "");
	nzbget_rs_free(cacheFlagBuf);

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		int id = 0;
		char suffix = '\0';
		if (nzbget_rs_dqh_diskstate_parse_queue_file_id_and_suffix(filename, &id, &suffix) != 0)
		{
			if (suffix == 'c' || (g_Options->GetContinuePartial() && !cacheWasActive))
			{
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						if (fileInfo->GetId() == id)
						{
							// a damaged state file (cut short by a power loss: partial states
							// are written in place) costs only its own progress: the load left
							// blank articles behind, which then downloaded nothing, and the rest
							// of the queue dir's states weren't read
							int successArticles = fileInfo->GetSuccessArticles();
							int failedArticles = fileInfo->GetFailedArticles();
							int64 remainingSize = fileInfo->GetRemainingSize();
							int64 successSize = fileInfo->GetSuccessSize();
							int64 failedSize = fileInfo->GetFailedSize();
							if (!LoadFileState(fileInfo, servers, suffix == 'c'))
							{
								warn("Discarding damaged download state %s", filename);
								fileInfo->GetArticles()->clear();
								fileInfo->SetSuccessArticles(successArticles);
								fileInfo->SetFailedArticles(failedArticles);
								fileInfo->SetRemainingSize(remainingSize);
								fileInfo->SetSuccessSize(successSize);
								fileInfo->SetFailedSize(failedSize);
								NzbgetRsBuf dmgFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
									g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
								FileSystem::DeleteFile(dmgFnBuf.data ? dmgFnBuf.data : "");
								nzbget_rs_free(dmgFnBuf);
								goto next;
							}
							if (m_lastFileStateVersion > DISKSTATE_FILE_VERSION)
							{
								// an earlier build of this branch wrote it: rewritten in the
								// format an nzbget without duplicate repair reads too
								SaveFileState(fileInfo, suffix == 'c');
							}
							fileInfo->GetArticles()->clear();
							fileInfo->SetPartialState(suffix == 'c' ? FileInfo::psCompleted : FileInfo::psPartial);
							goto next;
						}
					}
				}
			}
			else
			{
				NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
				FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
				nzbget_rs_free(fullFnBuf);
			}
		}
		next:;
	}

	return true;
}

inline bool DiskState::SaveStats(Servers* servers, ServerVolumes* serverVolumes)
{
	debug("Saving stats to disk");

	StateFile stateFile("stats", DISKSTATE_STATS_VERSION, true);

	if (servers->empty())
	{
		stateFile.Discard();
		return true;
	}

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	// save server names
	SaveServerInfo(servers, *outfile);

	// save stat
	SaveVolumeStat(serverVolumes, *outfile);

	// now rename to dest file name
	return stateFile.FinishWrite();
}

inline bool DiskState::LoadStats(Servers* servers, ServerVolumes* serverVolumes, bool* perfectMatch)
{
	debug("Loading stats from disk");

	StateFile stateFile("stats", DISKSTATE_STATS_VERSION, true);

	if (!stateFile.FileExists())
	{
		return true;
	}

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	bool ok = false;
	int formatVersion = stateFile.GetFileVersion();

	if (!LoadServerInfo(servers, *infile, formatVersion, perfectMatch)) goto error;

	if (formatVersion >=2)
	{
		if (!LoadVolumeStat(servers, serverVolumes, *infile, formatVersion)) goto error;
	}

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for statistics");
	}

	return ok;
}

inline bool DiskState::SaveServerInfo(Servers* servers, StateDiskFile& outfile)
{
	debug("Saving server info to disk");

	std::vector<const char*> names;
	std::vector<const char*> hosts;
	std::vector<int> ports;
	std::vector<const char*> users;
	names.reserve(servers->size());
	hosts.reserve(servers->size());
	ports.reserve(servers->size());
	users.reserve(servers->size());
	for (NewsServer* newsServer : servers)
	{
		names.push_back(newsServer->GetName());
		hosts.push_back(newsServer->GetHost());
		ports.push_back(newsServer->GetPort());
		users.push_back(newsServer->GetUser());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_server_info(
		names.data(), hosts.data(), ports.data(), users.data(), names.size()));

	return true;
}

inline bool DiskState::LoadServerInfo(Servers* servers, StateDiskFile& infile, int formatVersion, bool* perfectMatch)
{
	debug("Loading server info from disk");

	*perfectMatch = true;

	int size = 0;
	std::vector<int> refIds;
	std::vector<CString> refNameStrs;
	std::vector<CString> refHostStrs;
	std::vector<int> refPorts;
	std::vector<CString> refUserStrs;
	if (infile.ScanLine("%i", &size) != 1 || size < 0) goto error;
	refIds.reserve(size);
	refNameStrs.reserve(size);
	refHostStrs.reserve(size);
	refPorts.reserve(size);
	refUserStrs.reserve(size);
	for (int i = 0; i < size; i++)
	{
		char name[1024];
		if (!infile.ReadLine(name, sizeof(name))) goto error;

		char host[200];
		if (!infile.ReadLine(host, sizeof(host))) goto error;

		int port;
		if (infile.ScanLine("%i", &port) != 1) goto error;

		char user[100];
		if (!infile.ReadLine(user, sizeof(user))) goto error;

		refIds.push_back(i + 1);
		refNameStrs.emplace_back(name);
		refHostStrs.emplace_back(host);
		refPorts.push_back(port);
		refUserStrs.emplace_back(user);
	}

	{
		std::vector<const char*> srvNames;
		std::vector<const char*> srvHosts;
		std::vector<int> srvPorts;
		std::vector<const char*> srvUsers;
		srvNames.reserve(servers->size());
		srvHosts.reserve(servers->size());
		srvPorts.reserve(servers->size());
		srvUsers.reserve(servers->size());
		for (NewsServer* newsServer : servers)
		{
			srvNames.push_back(newsServer->GetName());
			srvHosts.push_back(newsServer->GetHost());
			srvPorts.push_back(newsServer->GetPort());
			srvUsers.push_back(newsServer->GetUser());
		}

		std::vector<const char*> refNames;
		std::vector<const char*> refHosts;
		std::vector<const char*> refUsers;
		refNames.reserve(refNameStrs.size());
		refHosts.reserve(refHostStrs.size());
		refUsers.reserve(refUserStrs.size());
		for (size_t i = 0; i < refNameStrs.size(); i++)
		{
			refNames.push_back(*refNameStrs[i]);
			refHosts.push_back(*refHostStrs[i]);
			refUsers.push_back(*refUserStrs[i]);
		}

		std::vector<int> outStateIds(servers->size(), 0);
		*perfectMatch = nzbget_rs_dsp_match_servers(
			srvNames.data(), srvHosts.data(), srvPorts.data(), srvUsers.data(), servers->size(),
			refIds.data(), refNames.data(), refHosts.data(), refPorts.data(), refUsers.data(), refIds.size(),
			outStateIds.data()) != 0;

		size_t idx = 0;
		for (NewsServer* newsServer : servers)
		{
			newsServer->SetStateId(outStateIds[idx++]);
		}
	}

	debug("******** MATCHING NEWS-SERVERS **********");
	for (NewsServer* newsServer : servers)
	{
		*perfectMatch = *perfectMatch && newsServer->GetStateId();
		debug("Server %i -> %i", newsServer->GetId(), newsServer->GetStateId());
		debug("Server %i.Name: %s", newsServer->GetId(), newsServer->GetName());
		debug("Server %i.Host: %s:%i", newsServer->GetId(), newsServer->GetHost(), newsServer->GetPort());
	}

	debug("All servers perfectly matched: %s", *perfectMatch ? "yes" : "no");

	return true;

error:
	error("Error reading server info from disk");

	return false;
}

inline bool DiskState::SaveVolumeStat(ServerVolumes* serverVolumes, StateDiskFile& outfile)
{
	debug("Saving volume stats to disk");

	outfile.PrintLine("%i", (int)serverVolumes->size());
	for (ServerVolume& serverVolume : serverVolumes)
	{
		const auto& articlesPerDays = serverVolume.GetArticlesPerDays();
		std::vector<NzbgetRsRpcArticlePair> articleEntries;
		articleEntries.reserve(articlesPerDays.size());
		for (const ServerVolume::Articles& articles : articlesPerDays)
		{
			articleEntries.push_back(NzbgetRsRpcArticlePair{ articles.failed, articles.success });
		}

		const ServerVolume::VolumeArray* bps = serverVolume.BytesPerSeconds();
		const ServerVolume::VolumeArray* bpm = serverVolume.BytesPerMinutes();
		const ServerVolume::VolumeArray* bph = serverVolume.BytesPerHours();
		const ServerVolume::VolumeArray* bpd = serverVolume.BytesPerDays();

		NzbgetRsDiskVolumeStatInput volInput{
			serverVolume.GetFirstDay(),
			static_cast<int>(serverVolume.GetDataTime()),
			static_cast<int>(serverVolume.GetCustomTime()),
			static_cast<int>(serverVolume.GetCountersResetTime()),
			serverVolume.GetTotalBytes(),
			serverVolume.GetCustomBytes(),
			bps->data(), bps->size(),
			bpm->data(), bpm->size(),
			bph->data(), bph->size(),
			bpd->data(), bpd->size(),
			articleEntries.data(), articleEntries.size()
		};
		WriteRsBuf(outfile, nzbget_rs_diskstate_format_volume_stat_item(&volInput));
	}

	return true;
}

inline bool DiskState::LoadVolumeStat(Servers* servers, ServerVolumes* serverVolumes, StateDiskFile& infile, int formatVersion)
{
	debug("Loading volume stats from disk");

	int size;
	std::vector<int32_t> srvStateIds;
	std::vector<int32_t> srvIds;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	srvStateIds.reserve(servers->size());
	srvIds.reserve(servers->size());
	for (NewsServer* newsServer : servers)
	{
		srvStateIds.push_back(newsServer->GetStateId());
		srvIds.push_back(newsServer->GetId());
	}
	for (int i = 0; i < size; i++)
	{
		ServerVolume* serverVolume = nullptr;

		int slotIdx = nzbget_rs_dqh_diskstate_select_volume_server_slot(
			i, srvStateIds.data(), srvIds.data(), srvStateIds.size());
		if (slotIdx >= 0)
		{
			serverVolume = &serverVolumes->at(slotIdx);
		}

		int firstDay, dataTime, customTime, countersResetTime;
		uint32 High1, Low1, High2 = 0, Low2 = 0;
		if (formatVersion >= 4)
		{
			if (infile.ScanLine("%i,%i,%i,%i", &firstDay, &dataTime,&customTime, &countersResetTime) != 4) goto error;
			if (infile.ScanLine("%u,%u,%u,%u", &High1, &Low1, &High2, &Low2) != 4) goto error;

			if (serverVolume)
			{
				serverVolume->SetCustomTime(static_cast<time_t>(customTime));
				serverVolume->SetCountersResetTime(static_cast<time_t>(countersResetTime));
			}
		}
		else if (formatVersion == 3)
		{
			if (infile.ScanLine("%i,%i,%i", &firstDay, &dataTime,&customTime) != 3) goto error;
			if (infile.ScanLine("%u,%u,%u,%u", &High1, &Low1, &High2, &Low2) != 4) goto error;

			if (serverVolume)
			{
				serverVolume->SetCustomTime(static_cast<time_t>(customTime));
				time_t firstDayTime = static_cast<time_t>(firstDay * 86400);
				serverVolume->SetCountersResetTime(firstDayTime);
			}
		}
		else
		{
			if (infile.ScanLine("%i,%i", &firstDay, &dataTime) != 2) goto error;
			if (infile.ScanLine("%u,%u", &High1, &Low1) != 2) goto error;
		}
		if (serverVolume) serverVolume->SetFirstDay(firstDay);
		if (serverVolume) serverVolume->SetDataTime((time_t)dataTime);
		if (serverVolume) serverVolume->SetTotalBytes(Util::JoinInt64(High1, Low1));
		if (serverVolume) serverVolume->SetCustomBytes(Util::JoinInt64(High2, Low2));

		ServerVolume::VolumeArray* VolumeArrays[] = { 
			serverVolume ? serverVolume->BytesPerSeconds() : nullptr,
			serverVolume ? serverVolume->BytesPerMinutes() : nullptr,
			serverVolume ? serverVolume->BytesPerHours() : nullptr,
			serverVolume ? serverVolume->BytesPerDays() : nullptr };
		for (int k=0; k < 4; k++)
		{
			ServerVolume::VolumeArray* volumeArray = VolumeArrays[k];

			// the seconds, minutes and hours arrays have fixed sizes (the counters
			// index them by the clock), the days one grows: a count from the file
			// sized them, and a wrong one (damaged file) threw at every start or
			// left them too short for the counters
			int arrSize;
			if (infile.ScanLine("%i", &arrSize) != 1) goto error;
			int targetVolumeSize = 0;
			if (!nzbget_rs_dsp_calc_volume_slot_size(k, arrSize, &targetVolumeSize)) goto error;
			if (volumeArray) volumeArray->assign(targetVolumeSize, 0);

			for (int j = 0; j < arrSize; j++)
			{
				if (infile.ScanLine("%u,%u", &High1, &Low1) != 2) goto error;
				if (nzbget_rs_dqh_diskstate_can_store_volume_slot(
					volumeArray != nullptr ? 1 : 0,
					j,
					volumeArray ? volumeArray->size() : 0) != 0)
				{
					(*volumeArray)[j] = Util::JoinInt64(High1, Low1);
				}
			}
		}

		if (formatVersion >= 4)
		{
			uint32 arrSize;
			if (infile.ScanLine("%u", &arrSize) != 1) goto error;
			if (arrSize > 100000) goto error;

			ServerVolume::ArticlesArray articlesArr;
			articlesArr.reserve(arrSize);

			ServerVolume::Articles articles;
			while (arrSize--)
			{
				if (infile.ScanLine("%u,%u", &articles.failed, &articles.success) != 2) goto error;
				articlesArr.push_back(articles);
			}

			if (serverVolume) serverVolume->SetArticlesPerDays(std::move(articlesArr));
		}
	}

	return true;

error:
	error("Error reading volume stats from disk");

	return false;
}

inline void DiskState::WriteCacheFlag()
{
	NzbgetRsBuf flagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	BString<1024> flagFilename = flagBuf.data ? flagBuf.data : "";
	nzbget_rs_free(flagBuf);

	StateDiskFile outfile;
	if (!outfile.Open(flagFilename, StateDiskFile::omWrite))
	{
		error("Error saving diskstate: Could not create file %s", *flagFilename);
		return;
	}

	outfile.Close();
}

inline void DiskState::DeleteCacheFlag()
{
	NzbgetRsBuf flagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	FileSystem::DeleteFile(flagBuf.data ? flagBuf.data : "");
	nzbget_rs_free(flagBuf);
}

inline void DiskState::AppendNzbMessage(int nzbId, Message::EKind kind, const char* text)
{
	NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	BString<1024> logFilename = logBuf.data ? logBuf.data : "";
	nzbget_rs_free(logBuf);

	StateDiskFile outfile;
	if (!outfile.Open(logFilename, StateDiskFile::omAppend))
	{
		error("Error saving log: Could not create file %s", *logFilename);
		return;
	}

	time_t tm = Util::CurrentTime();
	time_t rawtime = tm + g_Options->GetTimeCorrection();

	BString<100> time;
	Util::FormatTime(rawtime, time, 100);

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_nzb_message_line(
		*time, (int)tm, (int)kind, text, LINE_ENDING));

	outfile.Close();
}

inline void DiskState::LoadNzbMessages(int nzbId, MessageList* messages)
{
	// Important:
	//   - Other threads may be writing into the log-file at any time;
	//   - The log-file may also be deleted from another thread;

	NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	BString<1024> logFilename = logBuf.data ? logBuf.data : "";
	nzbget_rs_free(logBuf);

	if (!FileSystem::FileExists(logFilename))
	{
		return;
	}

	StateDiskFile infile;
	if (!infile.Open(logFilename, StateDiskFile::omRead))
	{
		error("Error reading log: could not open file %s", *logFilename);
		return;
	}

	int id = 0;
	char line[1024];
	while (infile.ReadLine(line, sizeof(line)))
	{
		Util::TrimRight(line);

		int time = 0;
		int kindInt = (int)Message::mkError;
		size_t textOffset = 0;
		if (!nzbget_rs_dsp_parse_log_line(line, &time, &kindInt, &textOffset)) goto exit;
		messages->emplace_back(++id, (Message::EKind)kindInt, (time_t)time, line + textOffset);
	}

exit:
	infile.Close();
	return;
}

#ifndef NZBFILE_H
#define NZBFILE_H

#include <algorithm>
#include <set>

class NzbFile
{
public:
	NzbFile(const char* fileName, const char* category)
		: m_fileName{ fileName ? fileName : "" }
	{
		debug("Creating NZBFile");

		m_nzbInfo = std::make_unique<NzbInfo>();
		m_nzbInfo->SetFilename(fileName);
		m_nzbInfo->SetCategory(category);
		m_nzbInfo->BuildDestDirName();
	}

	bool Parse()
	{
		std::unique_ptr<NzbgetRsNzbFileParser, void (*)(NzbgetRsNzbFileParser*)> parser(
			nzbget_rs_nzbfile_parser_new(m_fileName.c_str()),
			nzbget_rs_nzbfile_parser_free);

		NzbgetRsCtype ct = RsCtype();
		NzbgetRsNzbFileSummary summary{};
		bool ok = nzbget_rs_nzbfile_parser_parse_file(
			parser.get(),
			g_Options ? g_Options->GetParIgnoreExt() : "",
			&ct,
			nullptr,
			[](void*) -> int
			{
				try
				{
					FileInfo tmp;
					return tmp.GetId();
				}
				catch (...)
				{
					return 0;
				}
			},
			[](const char* subject, const char* filename)
			{
				try
				{
					detail("Extracting a filename from Subject %s", subject);
					detail("Extracted Filename: %s", filename);
				}
				catch (...)
				{
				}
			},
			[](const char* const* filenames, size_t* indices, size_t count)
			{
				try
				{
					std::sort(indices, indices + count,
						[filenames](size_t a, size_t b)
						{
							return strcmp(filenames[a], filenames[b]) > 0;
						});
				}
				catch (...)
				{
				}
			},
			&summary) != 0;

		for (size_t i = 0; i < summary.messages_count; i++)
		{
			int kind = 0;
			const char* text = nullptr;
			if (nzbget_rs_nzbfile_parser_get_message(parser.get(), i, &kind, &text) && text)
			{
				m_nzbInfo->AddMessage(
					kind == 2 ? Message::mkError : Message::mkWarning,
					text);
			}
		}

		if (!ok)
		{
			return false;
		}

		m_nzbInfo->SetManyDupeFiles(summary.many_dupe_files != 0);
		m_nzbInfo->SetFileCount(summary.file_count);
		m_nzbInfo->SetTotalArticles(summary.total_articles);
		m_nzbInfo->SetFailedArticles(summary.failed_articles);
		m_nzbInfo->SetCurrentFailedArticles(summary.cur_failed_articles);
		m_nzbInfo->SetSize(summary.size);
		m_nzbInfo->SetRemainingSize(summary.remaining_size);
		m_nzbInfo->SetFailedSize(summary.failed_size);
		m_nzbInfo->SetCurrentFailedSize(summary.cur_failed_size);
		m_nzbInfo->SetParSize(summary.par_size);
		m_nzbInfo->SetParFailedSize(summary.par_failed_size);
		m_nzbInfo->SetParCurrentFailedSize(summary.par_cur_failed_size);
		m_nzbInfo->SetRemainingParCount(summary.remaining_par_count);
		m_nzbInfo->SetFullContentHash(summary.full_content_hash);
		m_nzbInfo->SetFilteredContentHash(summary.filtered_content_hash);

		for (size_t i = 0; i < summary.kept_files_count; i++)
		{
			NzbgetRsNzbKeptFileView view{};
			if (!nzbget_rs_nzbfile_parser_get_file(parser.get(), i, &view))
			{
				continue;
			}
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>(view.id);
			fileInfo->SetNzbInfo(m_nzbInfo.get());
			fileInfo->SetSubject(view.subject ? view.subject : "");
			fileInfo->SetFilename(view.filename ? view.filename : "");
			fileInfo->SetTime(view.time);
			fileInfo->SetSize(view.size);
			fileInfo->SetRemainingSize(view.remaining_size);
			fileInfo->SetMissedSize(view.missed_size);
			fileInfo->SetTotalArticles(view.total_articles);
			fileInfo->SetMissedArticles(view.missed_articles);
			fileInfo->SetParFile(view.par_file != 0);

			for (size_t g = 0; g < view.groups_count; g++)
			{
				fileInfo->GetGroups()->push_back(view.groups_ptrs[g] ? view.groups_ptrs[g] : "");
			}

			ArticleList* articles = fileInfo->GetArticles();
			articles->reserve(view.articles_count);
			for (size_t a = 0; a < view.articles_count; a++)
			{
				std::unique_ptr<ArticleInfo> article = std::make_unique<ArticleInfo>();
				article->SetPartNumber(view.article_parts[a]);
				article->SetSize(view.article_sizes[a]);
				article->SetMessageId(view.article_msg_ids[a] ? view.article_msg_ids[a] : "");
				articles->push_back(std::move(article));
			}

			if (view.cut_short)
			{
				m_cutShortFiles.insert(fileInfo.get());
			}

			m_nzbInfo->GetFileList()->Add(std::move(fileInfo));
		}

		m_nzbInfo->UpdateMinMaxTime();

		if (g_Options && g_Options->GetServerMode())
		{
			for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
			{
				g_DiskState->SaveFile(fileInfo);
				fileInfo->GetArticles()->clear();
			}
		}

		m_password = summary.password ? summary.password : "";
		m_metaName = summary.meta_name ? summary.meta_name : "";
		m_metaTitle = summary.meta_title ? summary.meta_title : "";
		m_category = summary.category ? summary.category : "";

		if (!m_metaName.empty() && m_nzbInfo)
		{
			m_nzbInfo->SetMetaName(m_metaName);
		}

		return true;
	}

	const char* GetFileName() const { return m_fileName.c_str(); }
	const std::string& GetCategoryFromFile() const { return m_category; }
	std::unique_ptr<NzbInfo> DetachNzbInfo() { return std::move(m_nzbInfo); }
	const std::string& GetPassword() const { return m_password; }
	const std::string& GetMetaName() const { return m_metaName; }
	const std::string& GetMetaTitle() const { return m_metaTitle; }

	void LogDebugInfo() { info(" NZBFile %s", m_fileName.c_str()); }

	static int DeclaredParts(const char* subject)
	{
		NzbgetRsCtype ct = RsCtype();
		return nzbget_rs_nzb_declared_parts(subject, &ct);
	}

	const std::set<FileInfo*>& GetCutShortFiles() const { return m_cutShortFiles; }

private:
	static NzbgetRsCtype RsCtype()
	{
#ifdef __GLIBC__
		return {*__ctype_b_loc(), (unsigned short)_ISspace, (unsigned short)_ISdigit,
			reinterpret_cast<const int*>(*__ctype_tolower_loc()), CHAR_MIN < 0};
#else
		static thread_local unsigned short classes[256];
		static thread_local int lower[384];
		for (int c = 0; c < 256; c++)
		{
			classes[c] = (isspace(c) ? 1 : 0) | (isdigit(c) ? 2 : 0);
		}
		for (int c = -128; c < 256; c++)
		{
			lower[c + 128] = c < 0 ? c : tolower(c);
		}
		return {classes, 1, 2, lower + 128, CHAR_MIN < 0};
#endif
	}

	std::unique_ptr<NzbInfo> m_nzbInfo;
	std::string m_fileName;
	std::string m_category;
	std::string m_password;
	std::string m_metaName;
	std::string m_metaTitle;
	std::set<FileInfo*> m_cutShortFiles;
};

#endif

inline std::atomic<int> FileInfo::m_idGen{0};
inline int FileInfo::m_idMax = 0;
inline std::atomic<int> NzbInfo::m_idGen{0};
inline int NzbInfo::m_idMax = 0;
inline DownloadQueue* DownloadQueue::g_DownloadQueue = nullptr;
inline std::atomic<bool> DownloadQueue::g_Loaded{false};

inline void NzbParameterList::SetParameter(const char* name, const char* value)
{
	bool emptyVal = Util::EmptyStr(value);

	iterator pos = std::find_if(begin(), end(),
		[name](NzbParameter& parameter)
		{
			return !strcasecmp(parameter.GetName(), name);
		});

	if (emptyVal && pos != end())
	{
		erase(pos);
	}
	else if (!emptyVal)
	{
		if (pos != end())
		{
			pos->SetValue(value);
		}
		else
		{
			emplace_back(name, value);
		}
	}
}

inline NzbParameter* NzbParameterList::Find(const char* name)
{
	for (NzbParameter& parameter : this)
	{
		if (!strcasecmp(parameter.GetName(), name))
		{
			return &parameter;
		}
	}

	return nullptr;
}

inline void NzbParameterList::CopyFrom(NzbParameterList* sourceParameters)
{
	for (NzbParameter& parameter : sourceParameters)
	{
		SetParameter(parameter.GetName(), parameter.GetValue());
	}
}


inline ScriptStatus::EStatus ScriptStatusList::CalcTotalStatus()
{
	std::vector<int32_t> statuses;
	statuses.reserve(size());
	for (ScriptStatus& scriptStatus : this)
	{
		statuses.push_back((int32_t)scriptStatus.GetStatus());
	}
	return (ScriptStatus::EStatus)nzbget_rs_downloadinfo_script_calc_total_status(
		statuses.empty() ? nullptr : statuses.data(), statuses.size());
}


inline void ServerStatList::StatOp(int serverId, int successArticles, int failedArticles, EStatOperation statOperation)
{
	ServerStat* serverStat = nullptr;
	for (ServerStat& serverStat1 : this)
	{
		if (serverStat1.GetServerId() == serverId)
		{
			serverStat = &serverStat1;
			break;
		}
	}

	if (!serverStat)
	{
		emplace_back(serverId);
		serverStat = &back();
	}

	int newSuccess = serverStat->GetSuccessArticles();
	int newFailed = serverStat->GetFailedArticles();
	nzbget_rs_downloadinfo_server_stat_apply_op(
		newSuccess,
		newFailed,
		successArticles,
		failedArticles,
		(int)statOperation,
		(int)soSet,
		(int)soAdd,
		(int)soSubtract,
		&newSuccess,
		&newFailed);
	serverStat->SetSuccessArticles(newSuccess);
	serverStat->SetFailedArticles(newFailed);
}

inline void ServerStatList::ListOp(ServerStatList* serverStats, EStatOperation statOperation)
{
	for (ServerStat& serverStat : serverStats)
	{
		StatOp(serverStat.GetServerId(), serverStat.GetSuccessArticles(), serverStat.GetFailedArticles(), statOperation);
	}
}


inline void NzbInfo::SetId(int id)
{
	m_id = id;
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
}

inline void NzbInfo::ResetGenId(bool max)
{
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
}

inline int NzbInfo::GenerateId()
{
	return ++m_idGen;
}

inline void NzbInfo::SetUrl(const char* url)
{
	m_url = url;

	if (!m_name)
	{
		CString nzbNicename = MakeNiceUrlName(url, m_filename);
		SetName(nzbNicename);
	}
}

inline void NzbInfo::SetFilename(const char* filename)
{
	bool hadFilename = !Util::EmptyStr(m_filename);
	m_filename = filename;

	if ((!m_name || !hadFilename) && !Util::EmptyStr(filename))
	{
		CString nzbNicename = MakeNiceNzbName(m_filename, true);
		if (nzbNicename.Empty())
		{
			nzbNicename.Format("nzb_%i", GetId());
		}
		SetName(nzbNicename);
	}
}

inline CString NzbInfo::MakeNiceNzbName(const char* nzbFilename, bool removeExt)
{
	NzbgetRsBuf buf = nzbget_rs_downloadinfo_make_nice_nzb_name(nzbFilename, removeExt ? 1 : 0);
	std::string out(buf.data ? buf.data : "nzb", buf.len);
	nzbget_rs_free(buf);
	return out.c_str();
}

inline CString NzbInfo::MakeNiceUrlName(const char* urlStr, const char* nzbFilename)
{
	NzbgetRsBuf buf = nzbget_rs_downloadinfo_make_nice_url_name(urlStr, nzbFilename);
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out.c_str();
}

inline void NzbInfo::BuildDestDirName()
{
	if (Util::EmptyStr(g_Options->GetInterDir()))
	{
		m_destDir = BuildFinalDirName();
	}
	else
	{
		NzbgetRsBuf destBuf = nzbget_rs_downloadinfo_build_dest_dir_name(
			g_Options->GetInterDir(), nullptr, nullptr, nullptr, 0, GetName(), GetId());
		m_destDir = destBuf.data ? destBuf.data : "";
		nzbget_rs_free(destBuf);
	}
}

inline CString NzbInfo::BuildFinalDirName()
{
	const char* categoryDestDir = nullptr;
	if (!m_category.Empty())
	{
		Options::Category* category = g_Options->FindCategory(m_category, false);
		if (category && !Util::EmptyStr(category->GetDestDir()))
		{
			categoryDestDir = category->GetDestDir();
		}
	}

	NzbgetRsBuf finalBuf = nzbget_rs_downloadinfo_build_final_dir_name(
		g_Options->GetDestDir(),
		categoryDestDir,
		m_category,
		g_Options->GetAppendCategoryDir() ? 1 : 0,
		GetName(),
		GetId());
	CString finalDir = finalBuf.data ? finalBuf.data : "";
	nzbget_rs_free(finalBuf);
	return finalDir;
}

inline int NzbInfo::CalcHealth()
{
	return nzbget_rs_qhp_calc_health(m_size, m_parSize, m_currentFailedSize, m_parCurrentFailedSize);
}

inline bool NzbInfo::RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
{
	return nzbget_rs_qhp_rename_completed_file(
		m_completedFiles.size(),
		oldName,
		newAbsoluteOrRelativePath,
		&m_completedFiles,
		[](void* rawCtx, size_t idx, const char** outFilename, const char** outOrigname) {
			auto& list = *static_cast<CompletedFileList*>(rawCtx);
			*outFilename = list[idx].GetFilename();
			*outOrigname = list[idx].GetOrigname();
		},
		[](void* rawCtx, size_t idx, const char* newFilename, const char* newOrigname) {
			auto& list = *static_cast<CompletedFileList*>(rawCtx);
			if (newOrigname)
			{
				list[idx].SetOrigname(newOrigname);
			}
			list[idx].SetFilename(newFilename);
		}) != 0;
}

inline int NzbInfo::CalcCriticalHealth(bool allowEstimation)
{
	return nzbget_rs_qhp_calc_critical_health(
		m_size, m_parSize, m_parCurrentFailedSize, allowEstimation ? 1 : 0);
}

inline void NzbInfo::UpdateMinMaxTime()
{
	std::vector<int64_t> times;
	times.reserve(m_fileList.size());
	for (FileInfo* fileInfo : &m_fileList)
	{
		times.push_back((int64_t)fileInfo->GetTime());
	}
	int64_t minTime = 0, maxTime = 0;
	nzbget_rs_downloadinfo_update_min_max_time(
		times.empty() ? nullptr : times.data(), times.size(), &minTime, &maxTime);
	m_minTime = (time_t)minTime;
	m_maxTime = (time_t)maxTime;
}

inline void NzbInfo::AddMessage(Message::EKind kind, const char * text, bool print)
{
	if (print)
	{
		switch (kind)
		{
		case Message::mkDetail:
			detail("%s", text);
			break;

		case Message::mkInfo:
			info("%s", text);
			break;

		case Message::mkWarning:
			warn("%s", text);
			break;

		case Message::mkError:
			error("%s", text);
			break;

		case Message::mkDebug:
			debug("%s", text);
			break;
		}
	}

	Guard guard(m_logMutex);

	m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);

	if (g_Options->GetServerMode() && g_Options->GetNzbLog())
	{
		g_DiskState->AppendNzbMessage(m_id, kind, text);
		m_messageCount.fetch_add(1, std::memory_order_relaxed);
	}

	while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
	{
		m_messages.pop_front();
	}

	m_cachedMessageCount = m_messages.size();
}

inline void NzbInfo::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, format);
	vsnprintf(tmp2, 1024, format, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	AddMessage(kind, tmp2);
}

inline void NzbInfo::ClearMessages()
{
	Guard guard(m_logMutex);
	m_messages.clear();
	m_cachedMessageCount = 0;
}

inline void NzbInfo::MoveFileList(NzbInfo* srcNzbInfo)
{
	m_fileList = std::move(*srcNzbInfo->GetFileList());
	for (FileInfo* fileInfo : &m_fileList)
	{
		fileInfo->SetNzbInfo(this);
	}

	SetFullContentHash(srcNzbInfo->GetFullContentHash());
	SetFilteredContentHash(srcNzbInfo->GetFilteredContentHash());

	SetFileCount(srcNzbInfo->GetFileCount());
	SetPausedFileCount(srcNzbInfo->GetPausedFileCount());
	SetRemainingParCount(srcNzbInfo->GetRemainingParCount());

	SetSize(srcNzbInfo->GetSize());
	SetRemainingSize(srcNzbInfo->GetRemainingSize());
	SetPausedSize(srcNzbInfo->GetPausedSize());
	SetSuccessSize(srcNzbInfo->GetSuccessSize());
	SetCurrentSuccessSize(srcNzbInfo->GetCurrentSuccessSize());
	SetFailedSize(srcNzbInfo->GetFailedSize());
	SetCurrentFailedSize(srcNzbInfo->GetCurrentFailedSize());

	SetParSize(srcNzbInfo->GetParSize());
	SetParSuccessSize(srcNzbInfo->GetParSuccessSize());
	SetParCurrentSuccessSize(srcNzbInfo->GetParCurrentSuccessSize());
	SetParFailedSize(srcNzbInfo->GetParFailedSize());
	SetParCurrentFailedSize(srcNzbInfo->GetParCurrentFailedSize());

	SetTotalArticles(srcNzbInfo->GetTotalArticles());
	SetSuccessArticles(srcNzbInfo->GetSuccessArticles());
	SetFailedArticles(srcNzbInfo->GetFailedArticles());
	SetCurrentSuccessArticles(srcNzbInfo->GetSuccessArticles());
	SetCurrentFailedArticles(srcNzbInfo->GetFailedArticles());

	SetMinTime(srcNzbInfo->GetMinTime());
	SetMaxTime(srcNzbInfo->GetMaxTime());
}

inline void NzbInfo::EnterPostProcess()
{
	m_postInfo = std::make_unique<PostInfo>();
	m_postInfo->SetNzbInfo(this);
}

inline void NzbInfo::LeavePostProcess()
{
	m_postInfo.reset();
	m_unpackTried = false;
	m_lastUnpackStatus = 0;
	m_extractedArchives.clear();
	ClearMessages();
}

inline void NzbInfo::SetActiveDownloads(int activeDownloads)
{
	int outSec = m_downloadSec;
	int64 outStartTime = (int64)m_downloadStartTime;
	int outStartSec = m_downloadStartSec;
	int outChanged = 0;
	nzbget_rs_downloadinfo_calc_active_downloads_sec(
		m_activeDownloads, activeDownloads, m_kind == NzbInfo::nkNzb ? 1 : 0,
		(int64)Util::CurrentTime(), m_downloadSec, (int64)m_downloadStartTime,
		m_downloadStartSec, &outSec, &outStartTime, &outStartSec, &outChanged);
	m_downloadSec = outSec;
	m_downloadStartTime = (time_t)outStartTime;
	m_downloadStartSec = outStartSec;
	if (outChanged)
	{
		m_changed = true;
	}
	m_activeDownloads = activeDownloads;
}

inline bool NzbInfo::IsDupeSuccess()
{
	return nzbget_rs_downloadinfo_is_dupe_success(
		(int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_urlStatus, CalcHealth(),
		CalcCriticalHealth(true), CalcCriticalHealth(false),
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0) != 0;
}

inline const char* NzbInfo::MakeTextStatus(bool ignoreScriptStatus)
{
	int health = m_kind == NzbInfo::nkNzb ? CalcHealth() : 0;
	int criticalHealth = m_kind == NzbInfo::nkNzb ? CalcCriticalHealth(false) : 0;
	ScriptStatus::EStatus scriptStatus = (m_kind == NzbInfo::nkNzb && !ignoreScriptStatus)
		? m_scriptStatuses.CalcTotalStatus()
		: ScriptStatus::srSuccess;
	return nzbget_rs_downloadinfo_make_text_status(
		(int)m_kind, (int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_moveStatus, (int)m_urlStatus,
		(int)scriptStatus, health, criticalHealth);
}

inline void NzbInfo::UpdateCurrentStats()
{
	m_pausedFileCount = 0;
	m_remainingParCount = 0;
	m_remainingSize = 0;
	m_pausedSize = 0;
	m_currentSuccessArticles = m_successArticles;
	m_currentFailedArticles = m_failedArticles;
	m_currentSuccessSize = m_successSize;
	m_currentFailedSize = m_failedSize;
	m_parCurrentSuccessSize = m_parSuccessSize;
	m_parCurrentFailedSize = m_parFailedSize;
	m_extraPriority = 0;

	m_currentServerStats.ListOp(&m_serverStats, ServerStatList::soSet);

	for (FileInfo* fileInfo : &m_fileList)
	{
		NzbgetRsDpsCurrentStatsAcc acc{
			m_remainingSize,
			m_currentSuccessSize,
			m_currentFailedSize,
			m_pausedSize,
			m_parCurrentSuccessSize,
			m_parCurrentFailedSize,
			m_currentSuccessArticles,
			m_currentFailedArticles,
			m_extraPriority,
			m_pausedFileCount,
			m_remainingParCount,
			0
		};
		nzbget_rs_downloadinfo_update_current_stats_step(
			&acc,
			fileInfo->GetRemainingSize(),
			fileInfo->GetSuccessArticles(),
			fileInfo->GetFailedArticles(),
			fileInfo->GetSuccessSize(),
			fileInfo->GetFailedSize(),
			fileInfo->GetExtraPriority() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			fileInfo->GetParFile() ? 1 : 0);
		m_remainingSize = acc.remaining_size;
		m_currentSuccessArticles = acc.current_success_articles;
		m_currentFailedArticles = acc.current_failed_articles;
		m_currentSuccessSize = acc.current_success_size;
		m_currentFailedSize = acc.current_failed_size;
		m_extraPriority = acc.extra_priority;
		m_pausedFileCount = acc.paused_file_count;
		m_pausedSize = acc.paused_size;
		m_remainingParCount = acc.remaining_par_count;
		m_parCurrentSuccessSize = acc.par_current_success_size;
		m_parCurrentFailedSize = acc.par_current_failed_size;

		m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
	}
}

inline void NzbInfo::UpdateCompletedStats(FileInfo* fileInfo)
{
	nzbget_rs_downloadinfo_update_completed_stats(
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedSize(),
		fileInfo->GetFailedArticles(),
		fileInfo->GetSuccessArticles(),
		fileInfo->GetExtraPriority() ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		fileInfo->GetPaused() ? 1 : 0,
		&m_successSize,
		&m_failedSize,
		&m_failedArticles,
		&m_successArticles,
		&m_extraPriority,
		&m_parSuccessSize,
		&m_parFailedSize,
		&m_remainingParCount,
		&m_pausedFileCount);

	m_serverStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
}

inline void NzbInfo::UpdateDeletedStats(FileInfo* fileInfo)
{
	nzbget_rs_downloadinfo_update_deleted_stats(
		fileInfo->GetSize(),
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedSize(),
		fileInfo->GetMissedSize(),
		fileInfo->GetTotalArticles(),
		fileInfo->GetSuccessArticles(),
		fileInfo->GetFailedArticles(),
		fileInfo->GetMissedArticles(),
		fileInfo->GetRemainingSize(),
		fileInfo->GetExtraPriority() ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		fileInfo->GetPaused() ? 1 : 0,
		&m_fileCount,
		&m_size,
		&m_currentSuccessSize,
		&m_failedSize,
		&m_failedArticles,
		&m_currentFailedSize,
		&m_totalArticles,
		&m_currentSuccessArticles,
		&m_currentFailedArticles,
		&m_remainingSize,
		&m_extraPriority,
		&m_remainingParCount,
		&m_parSize,
		&m_parCurrentSuccessSize,
		&m_parFailedSize,
		&m_parCurrentFailedSize,
		&m_pausedFileCount,
		&m_pausedSize);

	m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
}

inline bool NzbInfo::IsDownloadCompleted(bool ignorePausedPars)
{
	return nzbget_rs_downloadinfo_is_download_completed(
		m_activeDownloads,
		ignorePausedPars ? 1 : 0,
		m_fileList.size(),
		&m_fileList,
		[](void* rawCtx, size_t idx, int* outPaused, int* outParFile, int* outDeleted) {
			auto* list = static_cast<FileList*>(rawCtx);
			FileInfo* fi = (*list)[idx].get();
			*outPaused = fi->GetPaused() ? 1 : 0;
			*outParFile = fi->GetParFile() ? 1 : 0;
			*outDeleted = fi->GetDeleted() ? 1 : 0;
		}) != 0;
}

inline void ArticleInfo::AttachSegment(std::unique_ptr<SegmentData> content, int64 offset, int size)
{
	m_segmentContent = std::move(content);
	m_segmentOffset = offset;
	m_segmentSize = size;
}

inline void ArticleInfo::DiscardSegment()
{
	m_segmentContent.reset();
}


inline void FileInfo::SetId(int id)
{
	m_id = id;
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
}

inline void FileInfo::ResetGenId(bool max)
{
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
}

inline void FileInfo::SetPaused(bool paused)
{
	if (m_paused != paused && m_nzbInfo)
	{
		m_nzbInfo->SetPausedFileCount(m_nzbInfo->GetPausedFileCount() + (paused ? 1 : -1));
		m_nzbInfo->SetPausedSize(m_nzbInfo->GetPausedSize() + (paused ? m_remainingSize : -m_remainingSize));
	}
	m_paused = paused;
}

inline void FileInfo::SetExtraPriority(bool extraPriority)
{
	if (m_extraPriority != extraPriority && m_nzbInfo)
	{
		m_nzbInfo->SetExtraPriority(m_nzbInfo->GetExtraPriority() + (extraPriority ? 1 : -1));
	}
	m_extraPriority = extraPriority;
}

inline void FileInfo::MakeValidFilename()
{
	m_filename = FileSystem::MakeValidFilename(m_filename.c_str()).Str();
}

inline void FileInfo::SetActiveDownloads(int activeDownloads)
{
	m_activeDownloads = activeDownloads;

	if (m_activeDownloads > 0 && !m_outputFileMutex)
	{
		m_outputFileMutex = std::make_unique<Mutex>();
	}
	else if (m_activeDownloads == 0 && m_outputFileMutex)
	{
		m_outputFileMutex.reset();
	}
}

inline bool FileInfo::IsHardLinked()
{
	return !m_hardLinkPath.empty();
}


inline CompletedFile::CompletedFile(int id, std::string filename, std::string origname, EStatus status,
	uint32 crc, bool parFile, std::string hash16k, std::string parSetId) 
	: m_id(id)
	, m_filename(std::move(filename))
	, m_origname(std::move(origname))
	, m_status(status)
	, m_crc(crc)
	, m_parFile(parFile)
	, m_hash16k(std::move(hash16k))
	, m_parSetId(std::move(parSetId))
{
	if (FileInfo::m_idMax < m_id)
	{
		FileInfo::m_idMax = m_id;
	}
}

inline bool CompletedFile::SameFilename(const char* name) const
{
	return nzbget_rs_downloadinfo_completed_same_filename(m_filename.c_str(), name) != 0;
}

inline void DupInfo::SetId(int id)
{
	m_id = id;
	if (NzbInfo::m_idMax < m_id)
	{
		NzbInfo::m_idMax = m_id;
	}
}


inline HistoryInfo::~HistoryInfo()
{
	if ((m_kind == hkNzb || m_kind == hkUrl) && m_info)
	{
		delete (NzbInfo*)m_info;
	}
	else if (m_kind == hkDup && m_info)
	{
		delete (DupInfo*)m_info;
	}
}

inline int HistoryInfo::GetId()
{
	if ((m_kind == hkNzb || m_kind == hkUrl))
	{
		return ((NzbInfo*)m_info)->GetId();
	}
	else // if (m_eKind == hkDup)
	{
		return ((DupInfo*)m_info)->GetId();
	}
}

inline const char* HistoryInfo::GetName()
{
	if (m_kind == hkNzb || m_kind == hkUrl)
	{
		return GetNzbInfo()->GetName();
	}
	else if (m_kind == hkDup)
	{
		return GetDupInfo()->GetName();
	}
	else
	{
		return "<unknown>";
	}
}


inline void DownloadQueue::CalcRemainingSize(int64* remaining, int64* remainingForced)
{
	int64 remainingSize = 0;
	int64 remainingForcedSize = 0;

	for (NzbInfo* nzbInfo : &m_queue)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (!fileInfo->GetPaused() && !fileInfo->GetDeleted())
			{
				remainingSize += fileInfo->GetRemainingSize();
				if (nzbInfo->GetForcePriority())
				{
					remainingForcedSize += fileInfo->GetRemainingSize();
				}
			}
		}
	}

	*remaining = remainingSize;

	if (remainingForced)
	{
		*remainingForced = remainingForcedSize;
	}
}


#endif /* DOWNLOADINFO_H */

#ifndef DUPEARTICLEFALLBACK_H
#define DUPEARTICLEFALLBACK_H
#include <map>
#include <set>
#include <vector>

/*
 * Recovers articles missing on all news servers from duplicate collections
 * (same title or dupe-key) found in download queue or history. The failed
 * article is requeued with the message-id of the equivalent article of the
 * duplicate; residual damage from slightly different segment boundaries is
 * left to par-repair. See option <DupeArticleFallback>.
 *
 * Once a file has recovered enough articles from duplicates, it "cuts over":
 * subsequent articles lead with the duplicate instead of failing on the
 * primary first (which would waste a full server sweep per article). The
 * primary remains a revert source in case the duplicate is the one missing
 * that particular article.
 *
 * The duplicate a file leads with rotates as well, independently of the
 * cutover: whenever duplicates are tried at all, the file's lead duplicate is
 * tried first, and when that lead misses enough articles in a row it is
 * demoted and the next duplicate takes over the lead for fresh articles, so a
 * holed lead does not cost a wasted fetch per article when other duplicates
 * could serve them. The rotation is bounded: once every duplicate has led
 * without a single lead success it stops (the duplicates are equally holed
 * and rotating - and logging - would be pure noise) until a lead success
 * resets it.
 *
 * An article's source order is PINNED at its first fallback (PinSources): the
 * donors known at that moment, rotated to the file's lead, primary revert
 * inserted when cut over. The fallback round indexes into that pinned list,
 * so neither a lead rotation nor a queue/history change while the article is
 * in flight can shift the round->source mapping under it (which would skip or
 * repeat sources). The lead is identified by donor nzb-id, never by list
 * position: a lead-round result only counts towards demotion while the pinned
 * slot-0 donor is still the file's lead, so a burst of stale failures cannot
 * cascade-demote donors that were never tried, and a donor's streak can never
 * be charged to another donor even when candidate lists differ per part.
 *
 * All methods must be called within DownloadQueue-lock.
 */
class DupeArticleFallback
{
public:
	// after this many articles of a file are recovered from duplicates, lead
	// with the duplicate for the file's remaining articles (cutover)
	static constexpr int CutoverThreshold = 3;
	// after this many consecutive articles missed by the lead duplicate, the
	// next duplicate becomes the lead for the file's fresh articles
	static constexpr int LeadDemoteThreshold = 3;

	bool TryFallback(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* PAR2 identity is not established by filenames, sizes, or partial byte
	 * matches. Recognized parity must never be a duplicate target or donor. */
	static bool IsParFile(FileInfo* fileInfo);
	/* what the file's first article holds, read from disk if not known yet */
	static FileInfo::EFirstContent FirstContent(FileInfo* fileInfo);
	/* does the collection have par2 files (the only check a borrowed article's bytes get) */
	static bool HasPar2(NzbInfo* nzbInfo);

	/* Defer download-time donor recovery while parity is being discovered or
	 * a known PAR set has not failed ordinary repair. Must be called within
	 * DownloadQueue-lock, like TryFallback. */
	static bool ShouldDeferToPar(NzbInfo* nzbInfo);

	/* True when the collection's damaged data already exceeds all of its par2
	 * recovery data, or is projected to (B59): once ProjectionSample articles
	 * were tried, the failed share of the data tried so far, applied to all of
	 * its data, plus ParEdgeMargin (left out when withMargin is false: the
	 * 40-in-a-row rule), exceeds the par2 recovery data. Par-check
	 * can't be counted on then; download-time article fallback stops deferring.
	 * Must be called within DownloadQueue-lock. */
	static bool ParCannotCover(NzbInfo* nzbInfo, bool withMargin = true);
	// articles tried before the damage of a download is projected
	static constexpr int ProjectionSample = 200;
	// damage within this share of the data (per mille) of all that par2 can repair
	// is not left to par-check: the projection is rough and par2 needs whole blocks
	static constexpr int ParEdgeMargin = 50;
	// the largest par2 block size the check of borrowed articles reads (256 MiB)
	static constexpr int64 MaxPar2BlockSize = 256LL * 1024 * 1024;

	/* Finds the file of the duplicate collection which corresponds to the target
	 * data file: preferably by filename, otherwise by unambiguous structural identity
	 * (article count and sizes). Returns nullptr if no or multiple candidates. */
	static FileInfo* MatchDonorFile(FileInfo* targetFile, NzbInfo* donorNzb,
		const char* targetNzbFilename = nullptr, int donorNzbId = 0, const char* donorPrint = nullptr,
		bool twinOnly = false);
	/* The step fingerprint as a 64-bit hash (0 = the file has too few or too
	 * uniform articles to be identified this way), for pairing after the
	 * target's article list is gone. */
	static uint64 ArticleSizeStepsHash(FileInfo* fileInfo);
	static const char* FindDonorMessageId(FileInfo* donorFile, int partNumber);

	/* Rotates the donor list so the donor with the given nzb-id comes first
	 * (order among the others preserved); no-op when the id is 0 or absent. */
	static void RotateToLead(RawNzbList& donors, int leadNzbId);

	/* Pure pinning step: derives the article's lead bookkeeping (slot-0 donor,
	 * next-lead donor, distinct donor count) from the collected candidates and
	 * their contributing donor nzb-ids, lazily fixes the file's lead to the
	 * slot-0 donor when still undecided, and stores the ordered sources on the
	 * article. candidates/contributors run parallel (one contributor id per
	 * candidate, the donor that FIRST offered that message-id). */
	static void FinishPin(FileInfo* fileInfo, ArticleInfo* articleInfo,
		const std::vector<CString>& candidates, const std::vector<int>& contributors,
		bool cutover, const char* primaryMessageId);

	/* Counts a failed fetch of the article's pinned lead donor towards the
	 * file's consecutive lead-miss streak. Only a lead-round (round 1) result
	 * counts, and only while the pinned slot-0 donor is still the file's
	 * lead. Returns true when the streak reached LeadDemoteThreshold and the
	 * lead was rotated to the article's next donor (requires another donor
	 * and an unexhausted rotation budget; the caller logs the switch). Also
	 * called when a provisionally-accepted lead article is demoted later
	 * (QueueCoordinator::DemoteFinishedArticle): that fetch was in truth a
	 * lead miss and must not leave its bogus streak reset standing. */
	static bool RegisterLeadFailure(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Ends the file's lead-miss streak after a successful fetch from the
	 * current lead (same gating as RegisterLeadFailure). The rotation budget
	 * is re-armed only when the success is verifiable (the article's decoded
	 * boundaries were pinned by finished neighbours): an article accepted
	 * provisionally may be demoted again later, and its bogus success must
	 * not bypass the rotation bound. */
	static void RegisterLeadSuccess(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* A lead donor deleted from queue/history would freeze the lead
	 * accounting for good: nzb-ids are never reused, so its id could never
	 * match a collected donor again and the snapshot gate would block
	 * RegisterLeadFailure/Success for every fresh pin. Treat such a lead as
	 * vacated - the next pin re-decides the lead and the accounting starts
	 * clean. */
	static void VacateGhostLead(FileInfo* fileInfo, const RawNzbList& donors);

	/* Parses a collection's retained queued .nzb from disk into a standalone
	 * NzbInfo (with article lists loaded back in server mode). Returns nullptr
	 * if the file is missing or unparseable. Unlike GetParsedDonor this does
	 * not use or fill the per-instance donor cache; safe to call without the
	 * DownloadQueue lock. */
	static std::unique_ptr<NzbInfo> ParseDonorNzb(const char* queuedFilename);

	/* Decoded byte offset the article must begin at, derived from currently
	 * known geometry: 0 for the first article of the file, the end of an
	 * already-finished preceding article otherwise. -1 = not derivable yet.
	 * Guards substituted donor articles against decoded-boundary drift: the
	 * structural gate compares posted (encoded) sizes only, so a donor with
	 * the same article count and total but non-uniform segmentation can pass
	 * it while its decoded boundaries drift at interior articles. */
	static int64 ExpectedSegmentOffset(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Decoded byte offset the article must end at (exclusive): the begin of an
	 * already-finished following article, or the decoded file size for the last
	 * article of the file. -1 = not derivable yet. */
	static int64 ExpectedSegmentEnd(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Verifies the decoded byte range recorded for a finished article
	 * (SegmentOffset/SegmentSize) exactly abuts the currently known geometry
	 * (see ExpectedSegmentOffset/ExpectedSegmentEnd). A substituted article
	 * failing this check would leave a zero-filled gap and/or overwrite a
	 * neighbour's bytes, so it must not count as a successful download. */
	/* B40: the borrowed articles of the completed file <path> whose bytes don't match the
	 * block checksums of its par2 set (the par2 files already in the download's
	 * directory); none if no par2 data describes the file. Reads the disk: call
	 * without the queue lock, on a file no downloader writes any more */
	static std::vector<ArticleInfo*> BorrowedPar2Mismatches(FileInfo* fileInfo, const char* path,
		const char* destDir, const char* filename);
	static bool SegmentAligned(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Whole-file decoded-boundary check, independent of the (non-persisted)
	 * fallback round. Returns the first finished article whose decoded range
	 * breaks the contiguous tiling of [0, DecodedFileSize) - the gap/overlap a
	 * mis-placed donor article leaves - or nullptr when the finished articles
	 * tile exactly OR the geometry cannot be judged (DecodedFileSize unknown /
	 * non-yEnc, an article without recorded placement, or an unfinished
	 * article). Meaningful only for a fully-downloaded yEnc file; used at
	 * completion to catch a provisionally-accepted drifted donor that survived
	 * a restart (reloaded as a plain finished article, so the round-gated
	 * checks no longer recognise it). Only meaningful for files that took donor
	 * bytes (see HasDonorArticles): a poster's own geometry may be unusual
	 * without being wrong, and such files must be written as without this
	 * feature. */
	static ArticleInfo* FirstUntiledArticle(FileInfo* fileInfo);

	/* True when any article of the file was substituted from a duplicate (its
	 * fallback round survives a restart through the staged flag of the file
	 * state). Files without donor articles are never judged by the geometry
	 * checks. */
	static bool HasDonorArticles(FileInfo* fileInfo);

	/* Folds the "=ybegin size=" of a successfully decoded article into the
	 * file's DecodedFileSize. Some posters randomise that field per article
	 * (the "=ypart" ranges stay correct); once two articles disagree the size
	 * is marked unknown (-1) for good, which turns off every size-based check
	 * for the file. Returns true only on the call that detects the
	 * disagreement. */
	static bool MergeDecodedFileSize(FileInfo* fileInfo, int64 articleFileSize);

private:
	// total declared size must match within 1/64 (~1.6%); the slack absorbs
	// differences in yEnc overhead (header line lengths, filenames)
	static constexpr int TotalSizeToleranceDiv = 64;
	// per-article declared size must match within 1/16 (~6%)
	static constexpr int PartSizeToleranceDiv = 16;
	// keep enough donors parsed to cover a whole dupe-set without re-parsing per
	// article; must comfortably exceed the number of duplicates of one release
	static constexpr int MaxCachedDonors = 16;
	// articles a file needs before its size steps are trusted as a fingerprint
	static constexpr int MinStepFingerprintArticles = 8;

	std::map<int, std::unique_ptr<NzbInfo>> m_parsedDonors;
	std::map<int, time_t> m_badDonors;	// donor id -> when its nzb-file couldn't be read
	static constexpr int BadDonorRetrySec = 600;
	static void AppendDonorCandidate(std::vector<CString>& candidates,
		std::vector<int>& contributors, int donorNzbId,
		NzbInfo* parsedDonor, FileInfo* targetFile, int partNumber,
		const char* targetNzbFilename = nullptr, const char* donorPrint = nullptr,
		bool twinOnly = false);
	static RawNzbList CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	// the subject filename of fileInfo in its own collection's nzb-file
	CString NzbFilenameOf(FileInfo* fileInfo);
	void PinSources(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo,
		bool twinOnly = false);
	NzbInfo* GetParsedDonor(NzbInfo* donorNzb);
};

#include <algorithm>
#include <atomic>
#include <climits>
#include <functional>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <vector>
#include "nzbget_rs.h"

class DupeStreamRepair
{
public:
	static constexpr int StreamFetchWorkers = 4;
	static constexpr int MaxStreamFetchWorkers = 16;
	static int StreamFetchWorkerCount(int primaryConnections)
	{
		return std::clamp(primaryConnections / 4, StreamFetchWorkers, MaxStreamFetchWorkers);
	}

	static StreamRangeList ComputeHoles(FileInfo* fileInfo);
	static int64 TotalSize(const StreamRangeList& ranges);
	static bool BuildRepairJob(FileInfo* fileInfo, const char* diskBasename);
	static std::string BuildDonorKey(NzbInfo* donorNzb, const char* password);
};

class ParDamage
{
public:
	struct Verdict
	{
		bool known = false;			// a damaged file matched a par2 set with its block size
		uint64 blockSize = 0;
		int damagedBlocks = 0;		// in the data files finished so far
		int recoveryBlocks = 0;		// in all par2 volumes of the download, counted whole
		int64 doneBytes = 0;		// data finished so far
		int64 totalBytes = 0;		// data of the download
		int projectedBlocks = 0;	// damagedBlocks at the rate so far, for all the data

		bool Certain() const { return known && damagedBlocks > recoveryBlocks; }
		bool Projected() const
		{
			return known && doneBytes * 5 >= totalBytes &&
				(int64)projectedBlocks * 4 > (int64)recoveryBlocks * 5 + 8;
		}
	};

	static void FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename);

	static Verdict Judge(NzbInfo* nzbInfo);

	static bool FirstReport(int nzbId)
	{
		return nzbget_rs_par_damage_first_report(nzbId) != 0;
	}

	static void Forget(int nzbId)
	{
		nzbget_rs_par_damage_forget(nzbId);
		nzbget_rs_streaming_par_forget(nzbId);
	}

	static int BlocksTouched(const StreamRangeList& holes, uint64 blockSize)
	{
		static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
			offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
			offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
			"StreamRange layout must match NzbgetRsByteRange");
		return nzbget_rs_par_damage_blocks_touched(
			holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
			holes.size(), blockSize);
	}
};

class TwinCheck : public Service
{
public:
	static constexpr const char* FilesParam = "DupeFiles";	// fingerprint, or "none"
	static constexpr const char* KindParam = "DupeKind";	// "twin" or "alt" (of the primary)
	static constexpr const char* SampledParam = "DupeSampled";		// "twin", "alt" or "none"
	static constexpr const char* SampledOfParam = "DupeSampledOf";	// the primary's id
	static constexpr int SampleCount = 6;
	static constexpr int64 MaxIndexSize = 16LL * 1024 * 1024;
	static constexpr int MaxRunning = 2;

	struct FileSig
	{
		std::string name;
		uint64 length = 0;
		std::string md5;		// hex, the whole file
		std::string hash16k;	// hex, its first 16 KB
	};

	struct NzbEntry
	{
		std::string subject;
		std::string filename;	// the quoted name in the subject, or ""
		std::vector<CString> groups;
		std::vector<std::pair<int64, std::string>> segments;	// bytes, message-id with brackets
		int64 size = 0;
		bool par2 = false;		// its name says .par2
	};
	static std::vector<NzbEntry> ReadNzbEntries(const char* filename);
	static bool FetchEntry(ArticleFetcher& fetcher, const NzbEntry& entry, int64 maxSize,
		const std::function<bool()>& stopped, std::vector<char>& data);
	static uint64 BlockSize(const char* data, size_t size);
	static int VolumeBlocks(const std::string& filename);

	static std::vector<FileSig> ParsePar2(const char* data, size_t size);
	static std::string Fingerprint(const std::vector<FileSig>& sigs);

	static std::string Posting(NzbInfo* nzbInfo);
	static std::string Identity(NzbInfo* nzbInfo);
	static std::string PutPar2(int nzbId, const char* posting, const char* data, size_t size,
		bool* complete = nullptr);
	static int HasSigs(int nzbId, const char* identity);	// 0 none, 1 partial, 2 complete
	static void Prune(const std::string& live);
	static std::string Kind(int primaryId, const char* primaryPrint, int dupeId, const char* dupePrint);
	static std::string MatchByContent(int ownId, const char* ownPrint, const char* target, const char* targetAlt,
		int donorId, const char* donorPrint);
	static bool Available()
	{
		return true;
	}

	static void StopAll();
	static void WaitAll();
	static void Reset();

	TwinCheck();
	~TwinCheck() override;

	static void Nudge();
	static void NudgeLocked();

protected:
	int ServiceInterval() override;
	void ServiceWork() override;

private:
	class DownloadQueueObserver final : public Observer
	{
	public:
		TwinCheck* m_owner;
		void Update(Subject* caller, void* aspect) override;
	};

	DownloadQueueObserver m_observer;
};


inline StreamRangeList DupeStreamRepair::ComputeHoles(FileInfo* fileInfo)
{
	int64 fileSize = fileInfo->GetDecodedFileSize();
	if (fileSize <= 0)
	{
		return StreamRangeList();
	}

	std::vector<NzbgetRsByteRange> covered;
	covered.reserve(fileInfo->GetArticles()->size());
	for (ArticleInfo* article : fileInfo->GetArticles())
	{
		if (article->GetStatus() == ArticleInfo::aiFinished && article->GetSegmentSize() > 0)
		{
			covered.push_back({article->GetSegmentOffset(), article->GetSegmentSize()});
		}
	}
	std::vector<NzbgetRsByteRange> rsHoles(covered.size() + 1);
	size_t count = nzbget_rs_stream_compute_holes(
		covered.data(), covered.size(), fileSize, rsHoles.data(), rsHoles.size());
	StreamRangeList holes;
	holes.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		holes.push_back({rsHoles[i].offset, rsHoles[i].size});
	}
	return holes;
}

inline int64 DupeStreamRepair::TotalSize(const StreamRangeList& ranges)
{
	return nzbget_rs_stream_total_size(
		reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size());
}

inline bool DupeStreamRepair::BuildRepairJob(FileInfo* fileInfo, const char* diskBasename)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	int jobMode = nzbget_rs_stream_eval_build_repair_job(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafStream,
		g_Options->GetRawArticle() ? 1 : 0,
		DupeArticleFallback::IsParFile(fileInfo) ? 1 : 0,
		diskBasename,
		nzbInfo ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetDeleteStatus() : 0,
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		nzbInfo && nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo && nzbInfo->GetParking() ? 1 : 0,
		nzbInfo && nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		fileInfo->GetArticles()->empty() ? 1 : 0,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSize(),
		fileInfo->GetDecodedFileSize());
	if (jobMode == 0)
	{
		return false;
	}

	if (jobMode == 1)
	{
		nzbInfo->GetStreamRepairJobs()->emplace_back(fileInfo->GetId(), diskBasename,
			0, fileInfo->GetFailedSize(), fileInfo->GetMissedSize(),
			fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles(), fileInfo->GetParFile(),
			StreamRangeList(), DupeArticleFallback::ArticleSizeStepsHash(fileInfo));
		return true;
	}

	StreamRangeList holes = ComputeHoles(fileInfo);
	if (holes.empty())
	{
		return false;
	}

	nzbInfo->GetStreamRepairJobs()->emplace_back(fileInfo->GetId(), diskBasename,
		fileInfo->GetDecodedFileSize(), fileInfo->GetFailedSize(), fileInfo->GetMissedSize(),
		fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles(), fileInfo->GetParFile(),
		std::move(holes), DupeArticleFallback::ArticleSizeStepsHash(fileInfo));

	return true;
}

inline std::string DupeStreamRepair::BuildDonorKey(NzbInfo* donorNzb, const char* password)
{
	size_t fileCount = donorNzb->GetFileList()->size();
	std::vector<const char*> fileNames;
	std::vector<int64_t> fileSizes;
	std::vector<uint8_t> fileIsPar;
	std::vector<size_t> fileGroupCounts;
	std::vector<const char*> groups;
	std::vector<size_t> fileArticleCounts;
	std::vector<int32_t> artPartNumbers;
	std::vector<int32_t> artSizes;
	std::vector<const char*> artMessageIds;
	fileNames.reserve(fileCount);
	fileSizes.reserve(fileCount);
	fileIsPar.reserve(fileCount);
	fileGroupCounts.reserve(fileCount);
	fileArticleCounts.reserve(fileCount);
	for (FileInfo* file : donorNzb->GetFileList())
	{
		fileNames.push_back(file->GetFilename());
		fileSizes.push_back(file->GetSize());
		fileIsPar.push_back(file->GetParFile() ? 1 : 0);
		fileGroupCounts.push_back(file->GetGroups()->size());
		for (const CString& group : *file->GetGroups())
		{
			groups.push_back(*group);
		}
		fileArticleCounts.push_back(file->GetArticles()->size());
		for (const std::unique_ptr<ArticleInfo>& article : *file->GetArticles())
		{
			artPartNumbers.push_back(article->GetPartNumber());
			artSizes.push_back(article->GetSize());
			artMessageIds.push_back(article->GetMessageId());
		}
	}
	NzbgetRsBuf buf = nzbget_rs_stream_build_donor_key(
		password,
		fileNames.data(),
		fileSizes.data(),
		fileIsPar.data(),
		fileGroupCounts.data(),
		fileCount,
		groups.data(),
		groups.size(),
		fileArticleCounts.data(),
		artPartNumbers.data(),
		artSizes.data(),
		artMessageIds.data(),
		artMessageIds.size());
	std::string key(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return key;
}

inline void ParDamage::FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename)
{
	static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
		offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
		offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
		"StreamRange layout must match NzbgetRsByteRange");
	bool isPar = fileInfo->GetParFile() || DupeArticleFallback::IsParFile(fileInfo);
	int failedPlusMissed = fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles();
	int64 decodedSize = fileInfo->GetDecodedFileSize();
	StreamRangeList holes;
	if (!isPar && failedPlusMissed > 0 && decodedSize > 0)
	{
		holes = DupeStreamRepair::ComputeHoles(fileInfo);
	}
	nzbget_rs_par_damage_file_completed(
		nzbInfo->GetId(),
		isPar ? 1 : 0,
		failedPlusMissed,
		decodedSize,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSize(),
		filename.c_str(),
		holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
		holes.size());

	if (isPar && failedPlusMissed == 0 && !filename.empty())
	{
		if (nzbget_rs_streaming_par_has_session(nzbInfo->GetId()) == 0)
		{
			if (nzbget_rs_streaming_par_start(nzbInfo->GetId(), filename.c_str(), nzbInfo->GetDestDir()) != 0)
			{
				for (FileInfo* qFile : nzbInfo->GetFileList())
				{
					if (!qFile || qFile == fileInfo || qFile->GetDeleted())
					{
						continue;
					}
					int baseLen = 0;
					int blocks = 0;
					if (ParParser::ParseParFilename(qFile->GetFilename(), qFile->GetFilenameConfirmed(), &baseLen, &blocks) &&
						blocks != 0 &&
						ParParser::SameParCollection(
							fileInfo->GetFilename(),
							qFile->GetFilename(),
							fileInfo->GetFilenameConfirmed() && qFile->GetFilenameConfirmed()))
					{
						nzbget_rs_streaming_par_register_queued_volume(
							nzbInfo->GetId(),
							static_cast<uint64_t>(qFile->GetId()),
							blocks > 0 ? blocks : 1,
							qFile->GetPaused() ? 1 : 0);
					}
				}
			}
		}
		else
		{
			nzbget_rs_streaming_par_on_recovery_volume_arrived(
				nzbInfo->GetId(),
				filename.c_str(),
				static_cast<uint64_t>(fileInfo->GetId()),
				1);
		}
	}
}

inline ParDamage::Verdict ParDamage::Judge(NzbInfo* nzbInfo)
{
	Verdict verdict;
	if (!nzbget_rs_par_damage_has_damage(nzbInfo->GetId()))
	{
		return verdict;
	}

	std::vector<NzbgetRsParDamageQueuedFile> queued;
	queued.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		queued.push_back({
			fileInfo->GetFilename(),
			fileInfo->GetFilenameConfirmed() ? 1 : 0,
			fileInfo->GetSize()
		});
	}

	std::vector<NzbgetRsParDamageCompletedFile> completed;
	completed.reserve(nzbInfo->GetCompletedFiles()->size());
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		completed.push_back({
			completedFile.GetFilename(),
			completedFile.GetStatus() != CompletedFile::cfFailure ? 1 : 0
		});
	}

	NzbgetRsParDamageVerdict rsVerdict{};
	nzbget_rs_par_damage_judge(
		nzbInfo->GetId(),
		nzbInfo->GetDestDir(),
		nzbInfo->GetSize(),
		nzbInfo->GetParSize(),
		queued.empty() ? nullptr : queued.data(),
		queued.size(),
		completed.empty() ? nullptr : completed.data(),
		completed.size(),
		&rsVerdict);

	verdict.known = rsVerdict.known != 0;
	verdict.blockSize = rsVerdict.block_size;
	verdict.damagedBlocks = rsVerdict.damaged_blocks;
	verdict.recoveryBlocks = rsVerdict.recovery_blocks;
	verdict.doneBytes = rsVerdict.done_bytes;
	verdict.totalBytes = rsVerdict.total_bytes;
	verdict.projectedBlocks = rsVerdict.projected_blocks;
	return verdict;
}

namespace TwinCheckDetail
{

inline std::string RsString(NzbgetRsBuf buf)
{
	std::string text(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return text;
}

inline bool OpenStore()
{
	static std::string openedPath;
	static std::mutex mutex;
	if (!g_Options || Util::EmptyStr(g_Options->GetQueueDir()))
	{
		return false;
	}
	std::string path = RsString(nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "twincheck"));
	std::lock_guard<std::mutex> guard(mutex);
	if (openedPath != path)
	{
		nzbget_rs_twin_open(path.c_str());
		openedPath = std::move(path);
	}
	return true;
}

class TwinCheckJob;

inline Mutex g_mutex;
inline std::set<TwinCheckJob*> g_jobs;
inline std::set<int> g_checking;
inline std::atomic<int> g_jobCount{0};
inline bool g_stopping = false;
inline std::map<int, int> g_failedAttempts;
inline constexpr int MaxAttempts = 3;
inline TwinCheck* g_instance = nullptr;
inline std::atomic<bool> g_nudged{false};

inline int Attempts(int nzbId)
{
	Guard guard(g_mutex);
	auto it = g_failedAttempts.find(nzbId);
	return it != g_failedAttempts.end() ? it->second : 0;
}

class TwinCheckJob : public Thread
{
public:
	TwinCheckJob(int nzbId, std::string nzbFilename, std::string posting) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_posting(std::move(posting)) {}
	TwinCheckJob(int nzbId, std::string nzbFilename, int primaryId, std::string primaryNzbFilename) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_primaryId(primaryId),
		m_primaryNzbFilename(std::move(primaryNzbFilename)) {}

	void Cancel() { m_fetcher.Stop(); }
	int GetNzbId() const { return m_nzbId; }

protected:
	void Run() override;

private:
	int m_nzbId;
	std::string m_nzbFilename;
	std::string m_posting;
	int m_primaryId = 0;
	std::string m_primaryNzbFilename;
	ArticleFetcher m_fetcher;
	bool m_stored = false;

	void Sample();
	void Store(const char* name, const char* value, const char* name2 = nullptr, const char* value2 = nullptr);
	bool IndexData(std::vector<char>& data, bool& fetched);
};

inline bool TwinCheckJob::IndexData(std::vector<char>& data, bool& fetched)
{
	fetched = false;
	std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(m_nzbFilename.c_str());

	std::vector<uint8_t> entryPar2(entries.size());
	std::vector<int64_t> entrySizes(entries.size());
	for (size_t i = 0; i < entries.size(); i++)
	{
		entryPar2[i] = entries[i].par2 ? 1 : 0;
		entrySizes[i] = entries[i].size;
	}
	ptrdiff_t idx = nzbget_rs_twin_select_min_par2_index(
		entryPar2.data(), entrySizes.data(), entries.size(), TwinCheck::MaxIndexSize);
	if (idx < 0 || (size_t)idx >= entries.size())
	{
		return false;
	}

	fetched = true;
	return TwinCheck::FetchEntry(m_fetcher, entries[(size_t)idx], TwinCheck::MaxIndexSize, [this]() { return IsStopped(); }, data);
}

inline void TwinCheckJob::Run()
{
	struct Unregister
	{
		TwinCheckJob* job;
		~Unregister()
		{
			Guard guard(g_mutex);
			g_jobs.erase(job);
			g_checking.erase(job->GetNzbId());
			g_jobCount--;
			if (!g_stopping && job->m_stored)
			{
				TwinCheck::NudgeLocked();
			}
		}
	} unregister{this};

	if (m_primaryId)
	{
		Sample();
		return;
	}

	std::vector<char> data;
	bool fetched = false;
	bool readable = IndexData(data, fetched);
	if (IsStopped())
	{
		return;
	}
	bool complete = false;
	std::string fingerprint = readable ?
		TwinCheck::PutPar2(m_nzbId, m_posting.c_str(), data.data(), data.size(), &complete) : "";
	bool known = !fingerprint.empty();
	if (fetched && (!known || !complete))
	{
		Guard guard(g_mutex);
		if (++g_failedAttempts[m_nzbId] < MaxAttempts)
		{
			return;
		}
	}
	if (!known)
	{
		fingerprint = fetched ? "lost" : "none";
	}
	int files = known ? nzbget_rs_twin_parse_fingerprint_files(fingerprint.c_str()) : 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(TwinCheck::FilesParam, fingerprint.c_str());
		m_stored = true;
		nzbInfo->PrintMessage(Message::mkDetail, "Fingerprint of %s: %s (%i file(s) in its par2 set)",
			nzbInfo->GetName(), fingerprint.c_str(), files);
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void TwinCheckJob::Store(const char* name, const char* value, const char* name2, const char* value2)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(name, value);
		if (name2)
		{
			nzbInfo->GetParameters()->SetParameter(name2, value2);
		}
		m_stored = true;
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void TwinCheckJob::Sample()
{
	auto dataFiles = [](const std::string& nzbFilename)
		{
			std::vector<TwinCheck::NzbEntry> files;
			for (TwinCheck::NzbEntry& entry : TwinCheck::ReadNzbEntries(nzbFilename.c_str()))
			{
				if (!entry.par2)
				{
					files.push_back(std::move(entry));
				}
			}
			std::sort(files.begin(), files.end(), [](const TwinCheck::NzbEntry& a, const TwinCheck::NzbEntry& b)
				{
					return nzbget_rs_twin_compare_entry_names(
						a.filename.c_str(), a.subject.c_str(),
						b.filename.c_str(), b.subject.c_str()) != 0;
				});
			return files;
		};
	std::vector<TwinCheck::NzbEntry> primary = dataFiles(m_primaryNzbFilename);
	std::vector<TwinCheck::NzbEntry> dupe = dataFiles(m_nzbFilename);
	std::string primaryId = std::to_string(m_primaryId);

	std::vector<size_t> pSegCounts;
	std::vector<int64> pSegBytes;
	pSegCounts.reserve(primary.size());
	for (const TwinCheck::NzbEntry& e : primary)
	{
		pSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			pSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> dSegCounts;
	std::vector<int64> dSegBytes;
	dSegCounts.reserve(dupe.size());
	for (const TwinCheck::NzbEntry& e : dupe)
	{
		dSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			dSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> sampleFileIdx(TwinCheck::SampleCount, 0);
	std::vector<size_t> sampleSegIdx(TwinCheck::SampleCount, 0);
	if (!nzbget_rs_twin_sample_indices(
		pSegCounts.data(), pSegBytes.data(), pSegBytes.size(), primary.size(),
		dSegCounts.data(), dSegBytes.data(), dSegBytes.size(), dupe.size(),
		(size_t)TwinCheck::SampleCount,
		sampleFileIdx.data(), sampleSegIdx.data()))
	{
		Store(TwinCheck::SampledParam, "none", TwinCheck::SampledOfParam, primaryId.c_str());
		return;
	}

	int compared = 0;
	int differ = 0;
	for (int n = 0; n < TwinCheck::SampleCount && !IsStopped(); n++)
	{
		const TwinCheck::NzbEntry& a = primary[sampleFileIdx[(size_t)n]];
		const TwinCheck::NzbEntry& b = dupe[sampleFileIdx[(size_t)n]];
		size_t segIdx = sampleSegIdx[(size_t)n];
		ArticleFetcher::FetchedArticle partA = m_fetcher.Fetch(a.segments[segIdx].second.c_str(), a.groups);
		ArticleFetcher::FetchedArticle partB = m_fetcher.Fetch(b.segments[segIdx].second.c_str(), b.groups);
		nzbget_rs_twin_compare_sampled_pair(
			partA.Success ? 1 : 0,
			partB.Success ? 1 : 0,
			partA.Offset,
			partB.Offset,
			partA.Data == partB.Data ? 1 : 0,
			compared,
			differ,
			&compared,
			&differ);
	}
	const char* label = "";
	int verdict = nzbget_rs_twin_eval_sample_verdict(
		IsStopped() ? 1 : 0, compared, differ, TwinCheck::SampleCount, &label);
	if (verdict == 0)
	{
		return;
	}
	Store(TwinCheck::SampledParam, label,
		TwinCheck::SampledOfParam, primaryId.c_str());
}

struct Item
{
	NzbInfo* nzbInfo;
	bool queued;
	int rank;
};

} // namespace TwinCheckDetail

inline std::vector<TwinCheck::NzbEntry> TwinCheck::ReadNzbEntries(const char* filename)
{
	std::vector<NzbEntry> entries;
	nzbget_rs_twin_visit_nzb_entries(
		filename,
		&entries,
		[](void* ctx, int64_t size, int32_t par2,
		   const char* fnamePtr, size_t fnameLen,
		   const char* subjPtr, size_t subjLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			NzbEntry entry;
			entry.size = size;
			entry.par2 = par2 != 0;
			entry.filename.assign(fnamePtr, fnameLen);
			entry.subject.assign(subjPtr, subjLen);
			out->push_back(std::move(entry));
		},
		[](void* ctx, const char* groupPtr, size_t groupLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().groups.emplace_back(std::string(groupPtr, groupLen).c_str());
			}
		},
		[](void* ctx, int64_t bytes, const char* idPtr, size_t idLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().segments.emplace_back(bytes, std::string(idPtr, idLen));
			}
		});
	return entries;
}

inline uint64 TwinCheck::BlockSize(const char* data, size_t size)
{
	return nzbget_rs_par2_block_size((const unsigned char*)data, size);
}

inline int TwinCheck::VolumeBlocks(const std::string& filename)
{
	return nzbget_rs_par2_volume_blocks(filename.c_str());
}

inline std::vector<TwinCheck::FileSig> TwinCheck::ParsePar2(const char* data, size_t size)
{
	std::vector<FileSig> sigs;
	nzbget_rs_twin_visit_par2_sigs(
		(const uint8_t*)data, size, &sigs,
		[](void* ctx, uint64_t length, const char* md5Ptr, size_t md5Len,
		   const char* namePtr, size_t nameLen) noexcept
		{
			auto* out = static_cast<std::vector<FileSig>*>(ctx);
			FileSig sig;
			sig.length = length;
			sig.md5.assign(md5Ptr, md5Len);
			sig.name.assign(namePtr, nameLen);
			out->push_back(std::move(sig));
		});
	return sigs;
}

inline std::string TwinCheck::Posting(NzbInfo* nzbInfo)
{
	return std::to_string((uint32_t)nzbInfo->GetFullContentHash());
}

inline std::string TwinCheck::Identity(NzbInfo* nzbInfo)
{
	NzbParameter* print = nzbInfo->GetParameters()->Find(FilesParam);
	if (!print)
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_format_identity(
		print->GetValue(), Posting(nzbInfo).c_str()));
}

inline std::string TwinCheck::PutPar2(int nzbId, const char* posting, const char* data, size_t size,
	bool* complete)
{
	if (complete)
	{
		*complete = false;
	}
	if (!data || !size || !TwinCheckDetail::OpenStore())
	{
		return "";
	}
	std::string result = TwinCheckDetail::RsString(nzbget_rs_twin_put_par2(nzbId, posting ? posting : "",
		(const unsigned char*)data, size));
	int32_t completeFlag = 0;
	std::string fp = TwinCheckDetail::RsString(nzbget_rs_twin_split_put_par2_result(
		result.c_str(), &completeFlag));
	if (complete)
	{
		*complete = completeFlag != 0;
	}
	return fp;
}

inline int TwinCheck::HasSigs(int nzbId, const char* print)
{
	return TwinCheckDetail::OpenStore() ? nzbget_rs_twin_has(nzbId, print) : 0;
}

inline void TwinCheck::Prune(const std::string& live)
{
	if (TwinCheckDetail::OpenStore())
	{
		nzbget_rs_twin_prune(live.c_str());
	}
}

inline std::string TwinCheck::Kind(int primaryId, const char* primaryPrint, int dupeId, const char* dupePrint)
{
	if (!TwinCheckDetail::OpenStore())
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_kind(primaryId, primaryPrint, dupeId, dupePrint));
}

inline std::string TwinCheck::MatchByContent(int ownId, const char* ownPrint, const char* target, const char* targetAlt,
	int donorId, const char* donorPrint)
{
	if (!TwinCheckDetail::OpenStore())
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_match(ownId, ownPrint, target, targetAlt, donorId, donorPrint));
}

inline std::string TwinCheck::Fingerprint(const std::vector<FileSig>& sigs)
{
	std::vector<const char*> md5s;
	std::vector<unsigned long long> lengths;
	md5s.reserve(sigs.size());
	lengths.reserve(sigs.size());
	for (const FileSig& sig : sigs)
	{
		md5s.push_back(sig.md5.c_str());
		lengths.push_back(sig.length);
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_fingerprint(md5s.data(), lengths.data(), sigs.size()));
}

inline bool TwinCheck::FetchEntry(ArticleFetcher& fetcher, const NzbEntry& entry, int64 maxSize,
	const std::function<bool()>& stopped, std::vector<char>& data)
{
	data.clear();
	for (const auto& segment : entry.segments)
	{
		if (stopped())
		{
			return false;
		}
		ArticleFetcher::FetchedArticle part = fetcher.Fetch(segment.second.c_str(), entry.groups);
		size_t newDataLen = data.size();
		if (!nzbget_rs_twin_is_fetched_entry_part_valid(
			part.Success ? 1 : 0, part.Offset, part.Data.size(),
			maxSize, data.size(), &newDataLen))
		{
			continue;
		}
		if (newDataLen > data.size())
		{
			data.resize(newDataLen);
		}
		std::copy(part.Data.begin(), part.Data.end(), data.begin() + (size_t)part.Offset);
	}
	return !data.empty();
}

inline void TwinCheck::ServiceWork()
{
	using namespace TwinCheckDetail;
	g_nudged = false;
	if (!Available() || !g_Options->GetDupeCheck())
	{
		return;
	}

	struct Job
	{
		int nzbId;
		std::string nzbFilename;
		bool hinted;
		int primaryId = 0;
		std::string primaryNzbFilename;
		std::string posting;
		int rank = 0;
	};
	std::vector<Job> jobs;
	std::string live;
	static time_t lastPrune = 0;
	bool prune = Util::CurrentTime() - lastPrune >= 60 * 60 && g_jobCount == 0;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (prune)
		{
			lastPrune = Util::CurrentTime();
			auto add = [&live](NzbInfo* nzbInfo)
				{
					std::string id = Identity(nzbInfo);
					if (!id.empty())
					{
						live += std::to_string(nzbInfo->GetId()) + "\t" + id + "\n";
					}
				};
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				add(nzbInfo);
			}
			for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb)
				{
					add(historyInfo->GetNzbInfo());
				}
			}
			if (live.empty())
			{
				live = "\n";
			}
		}
		bool changed = false;

		std::map<std::string, std::vector<Item>> keys;
		auto keyOf = [](NzbInfo* nzbInfo)
			{
				std::string k = nzbInfo->GetDupeKey() ? nzbInfo->GetDupeKey() : "";
				for (char& c : k)
				{
					c = (char)tolower((unsigned char)c);
				}
				return k;
			};
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetKind() == NzbInfo::nkNzb && !keyOf(nzbInfo).empty())
			{
				keys[keyOf(nzbInfo)].push_back({nzbInfo, true, 0});
			}
		}
		int rank = 0;
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			rank++;
			if (historyInfo->GetKind() == HistoryInfo::hkNzb && !keyOf(historyInfo->GetNzbInfo()).empty())
			{
				keys[keyOf(historyInfo->GetNzbInfo())].push_back({historyInfo->GetNzbInfo(), false, rank});
			}
		}

		auto isDupe = [](const Item& item)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				return nzbget_rs_twin_is_dupe_item(
					item.queued ? 1 : 0,
					nzbInfo->GetRemainingSize(),
					nzbInfo->GetPausedSize(),
					(int)nzbInfo->GetDeleteStatus(),
					(int)NzbInfo::dsDupe,
					(int)NzbInfo::dsCopy) != 0;
			};
		auto fingerprintOf = [](NzbInfo* nzbInfo)
			{
				NzbParameter* files = nzbInfo->GetParameters()->Find(FilesParam);
				return std::string(files ? files->GetValue() : "");
			};

		for (auto& entry : keys)
		{
			std::vector<Item>& items = entry.second;
			items.erase(std::remove_if(items.begin(), items.end(),
				[](const Item& item)
				{
					return strchr(item.nzbInfo->GetQueuedFilename(), '|') != nullptr;
				}),
				items.end());
			if (items.size() < 2)
			{
				continue;
			}
			size_t firstJob = jobs.size();
			int keyRank = INT_MAX;
			for (Item& item : items)
			{
				keyRank = std::min(keyRank, item.rank);
			}

			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				std::string print = fingerprintOf(nzbInfo);
				bool listless = nzbget_rs_twin_is_print_usable(print.c_str()) != 0 &&
					HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()) < 2 &&
					Attempts(nzbInfo->GetId()) < MaxAttempts;
				bool isMerged = strchr(nzbInfo->GetQueuedFilename(), '|') != nullptr;
				if ((print.empty() || listless) && !isMerged &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					bool hinted = nzbInfo->GetParameters()->Find("DupeSameSize") != nullptr ||
						nzbInfo->GetParameters()->Find("DupeSameInner") != nullptr;
					Job job{nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), hinted};
					job.posting = Posting(nzbInfo);
					jobs.push_back(std::move(job));
				}
			}

			NzbInfo* primary = nullptr;
			{
				std::vector<uint8_t> itemQueued;
				std::vector<uint8_t> itemIsDupe;
				std::vector<int> itemScores;
				itemQueued.reserve(items.size());
				itemIsDupe.reserve(items.size());
				itemScores.reserve(items.size());
				for (Item& item : items)
				{
					itemQueued.push_back(item.queued ? 1 : 0);
					itemIsDupe.push_back(isDupe(item) ? 1 : 0);
					itemScores.push_back(item.nzbInfo->GetDupeScore());
				}
				int primaryIdx = nzbget_rs_twin_select_primary(
					itemQueued.data(), itemIsDupe.data(), itemScores.data(), items.size());
				if (primaryIdx >= 0 && (size_t)primaryIdx < items.size())
				{
					primary = items[(size_t)primaryIdx].nzbInfo;
				}
			}

			std::string primaryPrint = primary ? fingerprintOf(primary) : "";
			bool primaryOk = primary &&
				strchr(primary->GetQueuedFilename(), '|') == nullptr &&
				FileSystem::FileExists(primary->GetQueuedFilename());
			std::string primaryId = primary ? std::to_string(primary->GetId()) : "";
			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				std::string print = fingerprintOf(nzbInfo);
				bool byPar2 = nzbget_rs_twin_is_print_usable(primaryPrint.c_str()) != 0 &&
					nzbget_rs_twin_is_print_usable(print.c_str()) != 0;
				NzbParameter* sampled = nzbInfo->GetParameters()->Find(SampledParam);
				NzbParameter* sampledOf = nzbInfo->GetParameters()->Find(SampledOfParam);
				int sampledNowInt = 0;
				bool sampleable = nzbget_rs_twin_eval_sampleable(
					primaryOk ? 1 : 0,
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					print.empty() ? 1 : 0,
					primaryPrint.empty() ? 1 : 0,
					byPar2 ? 1 : 0,
					sampled != nullptr ? 1 : 0,
					sampledOf ? sampledOf->GetValue() : nullptr,
					primaryId.c_str(),
					&sampledNowInt) != 0;
				bool sampledNow = sampledNowInt != 0;
				bool isMerged = strchr(nzbInfo->GetQueuedFilename(), '|') != nullptr;
				if (sampleable && !sampledNow && !isMerged &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					jobs.push_back({nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), false,
						primary->GetId(), primary->GetQueuedFilename()});
				}
				std::string byFiles;
				bool listed = false;
				if (nzbInfo != primary && isDupe(item) && byPar2)
				{
					byFiles = Kind(primary->GetId(), Identity(primary).c_str(), nzbInfo->GetId(),
						Identity(nzbInfo).c_str());
					listed = HasSigs(primary->GetId(), Identity(primary).c_str()) > 0 ||
						HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()) > 0;
				}
				std::string kindResolved = RsString(nzbget_rs_twin_resolve_kind(
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					byPar2 ? 1 : 0,
					byFiles.c_str(),
					listed ? 1 : 0,
					print == primaryPrint ? 1 : 0,
					sampleable ? 1 : 0,
					sampledNow ? 1 : 0,
					sampled ? sampled->GetValue() : ""));
				const char* kind = kindResolved.c_str();
				NzbParameter* old = nzbInfo->GetParameters()->Find(KindParam);
				if (nzbget_rs_dqh_bytes_equal(old ? old->GetValue() : "", kind) == 0)
				{
					nzbInfo->GetParameters()->SetParameter(KindParam, kind);
					changed = true;
					if (*kind)
					{
						std::string msg = RsString(nzbget_rs_twin_format_kind_message(
							nzbInfo->GetName(), primary->GetName(), kind, byPar2 ? 1 : 0));
						nzbInfo->PrintMessage(Message::mkInfo, "%s", msg.c_str());
					}
				}
			}

			for (size_t i = firstJob; i < jobs.size(); i++)
			{
				jobs[i].rank = keyRank;
			}
		}

		if (changed)
		{
			downloadQueue->HistoryChanged();
			downloadQueue->Save();
		}
	}

	if (!live.empty())
	{
		Guard guard(g_mutex);
		if (g_jobs.empty())
		{
			Prune(live);
		}
		else
		{
			lastPrune = 0;
		}
	}

	std::stable_sort(jobs.begin(), jobs.end(), [](const Job& a, const Job& b)
		{
			return a.rank != b.rank ? a.rank < b.rank : a.hinted > b.hinted;
		});

	Guard guard(g_mutex);
	for (Job& job : jobs)
	{
		if (g_stopping || (int)g_jobs.size() >= MaxRunning)
		{
			break;
		}
		if (g_checking.count(job.nzbId))
		{
			continue;
		}
		TwinCheckJob* thread = job.primaryId ?
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.primaryId, job.primaryNzbFilename) :
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.posting);
		g_jobs.insert(thread);
		g_checking.insert(job.nzbId);
		g_jobCount++;
		thread->SetAutoDestroy(true);
		thread->Start();
	}
}

inline void TwinCheck::StopAll()
{
	using namespace TwinCheckDetail;
	Guard guard(g_mutex);
	g_stopping = true;
	for (TwinCheckJob* job : g_jobs)
	{
		job->Stop();
		job->Cancel();
	}
}

inline void TwinCheck::WaitAll()
{
	while (TwinCheckDetail::g_jobCount > 0)
	{
		Util::Sleep(20);
	}
}

inline void TwinCheck::Reset()
{
	Guard guard(TwinCheckDetail::g_mutex);
	TwinCheckDetail::g_stopping = false;
}

inline TwinCheck::TwinCheck()
{
	Reset();
	{
		Guard guard(TwinCheckDetail::g_mutex);
		TwinCheckDetail::g_instance = this;
	}
	m_observer.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_observer);
}

inline TwinCheck::~TwinCheck()
{
	DownloadQueue::Guard()->Detach(&m_observer);
	Guard guard(TwinCheckDetail::g_mutex);
	TwinCheckDetail::g_instance = nullptr;
}

inline int TwinCheck::ServiceInterval()
{
	return TwinCheckDetail::g_nudged ? 1000 : 15000;
}

inline void TwinCheck::Nudge()
{
	Guard guard(TwinCheckDetail::g_mutex);
	NudgeLocked();
}

inline void TwinCheck::NudgeLocked()
{
	TwinCheckDetail::g_nudged = true;
	if (TwinCheckDetail::g_instance)
	{
		TwinCheckDetail::g_instance->WakeUp();
	}
}

inline void TwinCheck::DownloadQueueObserver::Update(Subject*, void* aspect)
{
	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaNzbAdded &&
		queueAspect->nzbInfo && !Util::EmptyStr(queueAspect->nzbInfo->GetDupeKey()))
	{
		TwinCheckDetail::g_nudged = true;
		m_owner->WakeUp();
	}
}

inline bool DupeArticleFallback::IsParFile(FileInfo* fileInfo)
{
	return nzbget_rs_dupe_fallback_is_par_file(
		fileInfo->GetParFile() ? 1 : 0,
		(int)fileInfo->GetFirstContent(),
		(int)FileInfo::fcPar2,
		fileInfo->GetFilenameConfirmed() ? 1 : 0,
		fileInfo->GetFilename()) != 0;
}

inline bool DupeArticleFallback::HasPar2(NzbInfo* nzbInfo)
{
	if (nzbInfo->GetParSize() > 0)
	{
		return true;
	}
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (IsParFile(fileInfo))
		{
			return true;
		}
	}
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_dupe_fallback_completed_file_is_par2(
			completedFile.GetParFile() ? 1 : 0, completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::ShouldDeferToPar(NzbInfo* nzbInfo)
{
	int headerVerdict = nzbget_rs_dupe_fallback_should_defer_to_par_header(
		nzbInfo ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetDirectRenameStatus() : 0,
		(int)NzbInfo::tsRunning,
		(nzbInfo && nzbInfo->GetAllFirst()) ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetParStatus() : 0,
		(int)NzbInfo::psFailure,
		nzbInfo ? nzbInfo->GetParSize() : 0);
	if (headerVerdict != 0)
	{
		return headerVerdict > 0;
	}
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (!fileInfo->GetDeleted() && IsParFile(fileInfo))
		{
			return true;
		}
	}
	for (CompletedFile& fileInfo : nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_dupe_fallback_completed_file_is_par2(
			fileInfo.GetParFile() ? 1 : 0, fileInfo.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::ParCannotCover(NzbInfo* nzbInfo, bool withMargin)
{
	return nzbget_rs_dupe_fallback_par_cannot_cover(
		nzbInfo->GetParSize(), nzbInfo->GetParCurrentFailedSize(),
		nzbInfo->GetCurrentFailedSize(), nzbInfo->GetCurrentSuccessSize(),
		nzbInfo->GetParCurrentSuccessSize(), nzbInfo->GetSize(),
		nzbInfo->GetCurrentSuccessArticles(), nzbInfo->GetCurrentFailedArticles(),
		withMargin ? 1 : 0) != 0;
}

inline bool DupeArticleFallback::TryFallback(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (g_Options->GetDupeArticleFallback() == Options::dafNone ||
		g_Options->GetRawArticle() ||
		IsParFile(fileInfo))
	{
		return false;
	}

	// a par2-file with a random name looks like any data file: what its first
	// article holds tells (that article itself may be borrowed: it's analyzed).
	// Until that's known only a name without an extension waits: one with an
	// extension of its own isn't a par2-file posing as data, and making every
	// file wait for its first article lost the articles failing before it
	FileInfo::EFirstContent content = FirstContent(fileInfo);
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	if (nzbget_rs_dupe_fallback_check_try_preconditions(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafNone,
		g_Options->GetRawArticle() ? 1 : 0,
		IsParFile(fileInfo) ? 1 : 0,
		(int)content,
		(int)FileInfo::fcPar2,
		(int)FileInfo::fcUnknown,
		articleInfo->GetPartNumber(),
		fileInfo->GetFilename(),
		fileInfo->GetDeleted() ? 1 : 0,
		nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo->GetParking() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce) == 0)
	{
		return false;
	}

	// A borrowed article proves only that it arrived intact (its yEnc crc), not that
	// it is this file's: a duplicate of the same size and article layout but other
	// bytes (another encode) filled the holes, and the download ended SUCCESS with
	// the wrong bytes. Its par2 files are what checks borrowed bytes (BorrowedPar2-
	// Mismatches, par-check): without any, holes are left to stream repair, which
	// proves a duplicate's bytes before it copies them
	if (!HasPar2(nzbInfo))
	{
		if (!nzbInfo->GetDupeNoParNoted())
		{
			nzbInfo->SetDupeNoParNoted(true);
			nzbInfo->PrintMessage(Message::mkInfo,
				"Not borrowing articles of %s from duplicates: it has no par2 files to check them",
				nzbInfo->GetName());
		}
		return false;
	}

	bool defer = false;
	if (g_Options->GetDupeArticleFallback() >= Options::dafStream)
	{
		// The first article identifies its file (the 16k hash direct-rename
		// and par-rename go by, the archive headers) - losing it costs far
		// more than the single donor fetch that can recover it.
		// Once the damage outgrows every recovery byte the collection has,
		// single articles are borrowed again (whole-file stream recovery
		// keeps waiting for par-check, which knows which files it protects).
		// Once lifted, the wait stays lifted: a projection can fall back below the
		// par2 data (articles fetched from a lead duplicate count as arrived).
		// A par-check that runs only when asked for (ParCheck=manual) repairs
		// nothing by itself, and the stream repair of post-processing waits for
		// it too: a hole left to it stayed, while the "article" mode had filled
		// it. Then the articles are borrowed as in that mode.
		bool parDefer = g_Options->GetParCheck() != Options::pcManual && ShouldDeferToPar(nzbInfo);
		ParDamage::Verdict damage = parDefer ? ParDamage::Judge(nzbInfo) : ParDamage::Verdict();
		// par2 repairs whole blocks: the blocks the failed articles spoiled may
		// be more than its volumes hold long before the bytes are (ParDamage);
		// borrowing then starts, and the par-damage failover waits for it
		int liftDeferState = 0;
		int logLifted = 0;
		defer = nzbget_rs_dupe_fallback_eval_par_defer(
			parDefer ? 1 : 0,
			articleInfo->GetPartNumber(),
			(int)nzbInfo->GetDupeParDeferState(),
			(int)NzbInfo::dpDeferred,
			(int)NzbInfo::dpLifted,
			ParCannotCover(nzbInfo) ? 1 : 0,
			damage.Certain() ? 1 : 0,
			damage.Projected() ? 1 : 0,
			&liftDeferState,
			&logLifted) != 0;
		if (liftDeferState)
		{
			if (logLifted)
			{
				nzbInfo->PrintMessage(Message::mkInfo,
					"Damage of %s exceeds its par2 recovery data (or will, judged by the articles tried so far), "
					"recovering missing articles from duplicates",
					nzbInfo->GetName());
			}
			nzbInfo->SetDupeParDeferState(NzbInfo::dpLifted);
		}
	}

	int round = articleInfo->GetDupeFallbackRound();
	if (nzbget_rs_dupe_fallback_should_abort_deferred_try(
		defer ? 1 : 0, round, downloadQueue ? 1 : 0,
		articleInfo->GetDupeSourceDonors()->empty() ? 1 : 0) != 0)
	{
		return false;
	}

	// preserve the article's own (primary) message-id before the first
	// substitution, so it can be used as a revert source when cut over
	if (Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()))
	{
		articleInfo->SetDupeOriginalMessageId(articleInfo->GetMessageId());
	}

	if (round == 0)
	{
		// the article's source order is decided once, at its first fallback:
		// the donors known now, rotated to the file's current lead, pinned on
		// the article so no later queue/history change or lead rotation can
		// shift the round->source mapping under it (which would skip or
		// repeat sources). The trade-off is deliberate: a duplicate appearing
		// later serves fresh articles, not articles already mid-fallback.
		// While deferring to par-check, only proven twins of this primary whose
		// par2 file list carries the exact same file bytes are pinned.
		PinSources(downloadQueue, fileInfo, articleInfo, defer);
	}
	else if (round == 1 && RegisterLeadFailure(fileInfo, articleInfo))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Switching lead duplicate collection for %s (the lead duplicate is missing many articles)",
			fileInfo->GetFilename());
	}

	std::vector<CString>& sources = *articleInfo->GetDupeSources();
	// sources of a duplicate whose articles failed the par2 checksums are passed over (B40)
	std::vector<int>& sourceDonors = *articleInfo->GetDupeSourceDonors();
	int pinnedRound = round;
	while (round >= 0 && (size_t)round < sources.size() && (size_t)round < sourceDonors.size() &&
		nzbInfo->GetDupeBlockedDonors()->count(sourceDonors[round]))
	{
		round++;
	}
	if (round >= (int)sources.size())
	{
		int checkLogParDefer = 0;
		int incUnsourced = 0;
		int restoreOrigMsgId = 0;
		nzbget_rs_dupe_fallback_resolve_exhausted_sources(
			defer ? 1 : 0,
			pinnedRound,
			articleInfo->GetDupeProactive() ? 1 : 0,
			(int)nzbInfo->GetDupeParDeferState(),
			(int)NzbInfo::dpNone,
			downloadQueue != nullptr ? 1 : 0,
			Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()) ? 1 : 0,
			&checkLogParDefer,
			&incUnsourced,
			&restoreOrigMsgId);
		if (checkLogParDefer && !CollectDonors(downloadQueue, nzbInfo).empty())
		{
			nzbInfo->SetDupeParDeferState(NzbInfo::dpDeferred);
			nzbInfo->PrintMessage(Message::mkInfo,
				"Deferring duplicate recovery for %s to par-check (its par2 files may cover the damage)",
				nzbInfo->GetName());
		}
		if (incUnsourced)
		{
			nzbInfo->SetDupeUnsourcedArticles(nzbInfo->GetDupeUnsourcedArticles() + 1);
		}
		if (restoreOrigMsgId)
		{
			articleInfo->SetMessageId(articleInfo->GetDupeOriginalMessageId());
		}
		return false;
	}

	// count this article once, the first time a duplicate source is tried for it
	// (the denominator of the per-file recovered/attempted completion summary).
	// No per-attempt log line: it would flood large files and, worse, read as a
	// success when it is only an attempt - the recovery is counted on success.
	// A duplicate asked first after the cutover isn't an attempt on a missing article:
	// counted, it read as "recovered 11 of 2853 missing" for a file missing ~20
	if (pinnedRound == 0 && !articleInfo->GetDupeProactive())
	{
		fileInfo->SetDupeAttemptedArticles(fileInfo->GetDupeAttemptedArticles() + 1);
		nzbInfo->SetDupeAttemptedArticles(nzbInfo->GetDupeAttemptedArticles() + 1);
	}

	// pin the decoded byte range this article must occupy, as far as it is
	// already known from finished neighbour articles, so a donor article with
	// drifted decoded boundaries is rejected before its bytes are written
	articleInfo->SetDupeExpectedOffset(ExpectedSegmentOffset(fileInfo, articleInfo));
	articleInfo->SetDupeExpectedEnd(ExpectedSegmentEnd(fileInfo, articleInfo));

	articleInfo->SetMessageId(sources[round]);
	articleInfo->SetDupeFallbackRound(round + 1);

	return true;
}

inline void DupeArticleFallback::RotateToLead(RawNzbList& donors, int leadNzbId)
{
	std::vector<int32_t> donorIds;
	donorIds.reserve(donors.size());
	for (NzbInfo* donor : donors)
	{
		donorIds.push_back(donor->GetId());
	}
	size_t idx = nzbget_rs_dupe_fallback_find_rotate_offset(
		donorIds.data(), donorIds.size(), leadNzbId);
	if (idx > 0 && idx < donors.size())
	{
		std::rotate(donors.begin(), donors.begin() + idx, donors.end());
	}
}

inline std::vector<ArticleInfo*> DupeArticleFallback::BorrowedPar2Mismatches(FileInfo* fileInfo, const char* path,
	const char* destDir, const char* filename)
{
	ArticleList* articles = fileInfo->GetArticles();
	size_t n = articles->size();
	if (n == 0)
	{
		return {};
	}
	std::vector<int32_t> statuses(n);
	std::vector<int32_t> donorIds(n);
	std::vector<int32_t> fallbackRounds(n);
	std::vector<int64_t> segmentOffsets(n);
	std::vector<int32_t> segmentSizes(n);
	for (size_t i = 0; i < n; i++)
	{
		ArticleInfo* a = (*articles)[i].get();
		statuses[i] = (int32_t)a->GetStatus();
		donorIds[i] = a->GetDupeDonorId();
		fallbackRounds[i] = a->GetDupeFallbackRound();
		segmentOffsets[i] = a->GetSegmentOffset();
		segmentSizes[i] = a->GetSegmentSize();
	}
	std::vector<int32_t> outIndices(n);
	size_t count = nzbget_rs_dupe_fallback_borrowed_par2_mismatches(
		path, destDir, filename, statuses.data(), donorIds.data(),
		fallbackRounds.data(), segmentOffsets.data(), segmentSizes.data(),
		n,
		[](const uint8_t* data, size_t len) -> uint32_t
		{
			Crc32 crc;
			crc.Append(const_cast<uchar*>(data), (uint32)len);
			return crc.Finish();
		},
		outIndices.data(), outIndices.size());
	std::vector<ArticleInfo*> mismatches;
	mismatches.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		int32_t idx = outIndices[i];
		if (idx >= 0 && (size_t)idx < n)
		{
			mismatches.push_back((*articles)[idx].get());
		}
	}
	return mismatches;
}

inline void DupeArticleFallback::FinishPin(FileInfo* fileInfo, ArticleInfo* articleInfo,
	const std::vector<CString>& candidates, const std::vector<int>& contributors,
	bool cutover, const char* primaryMessageId)
{
	int leadSnapshot = 0;
	int nextLead = 0;
	int donorCount = 0;
	int newFileLead = fileInfo->GetDupeLeadDonorId();
	nzbget_rs_dupe_fallback_compute_pin_lead_state(
		contributors.empty() ? nullptr : contributors.data(),
		contributors.size(),
		fileInfo->GetDupeLeadDonorId(),
		&leadSnapshot, &nextLead, &donorCount, &newFileLead);
	articleInfo->SetDupeLeadSnapshot(leadSnapshot);
	articleInfo->SetDupeNextLead(nextLead);
	articleInfo->SetDupeDonorCount(donorCount);
	fileInfo->SetDupeLeadDonorId(newFileLead);

	std::vector<CString>& sources = *articleInfo->GetDupeSources();
	sources.clear();
	bool appendPrimary = cutover && !candidates.empty();
	sources.reserve(candidates.size() + (appendPrimary ? 1 : 0));
	for (const CString& candidate : candidates)
	{
		sources.emplace_back((const char*)candidate);
	}
	if (appendPrimary)
	{
		sources.emplace_back(primaryMessageId);
	}
	*articleInfo->GetDupeSourceDonors() = contributors;
}

inline bool DupeArticleFallback::RegisterLeadFailure(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	int leadDonorId = fileInfo->GetDupeLeadDonorId();
	int leadFailures = fileInfo->GetDupeLeadFailures();
	int leadSwitches = fileInfo->GetDupeLeadSwitches();
	bool rotated = nzbget_rs_dupe_fallback_register_lead_failure(
		articleInfo->GetDupeFallbackRound(), articleInfo->GetDupeLeadSnapshot(),
		articleInfo->GetDupeDonorCount(), articleInfo->GetDupeNextLead(),
		&leadDonorId, &leadFailures, &leadSwitches) != 0;
	fileInfo->SetDupeLeadDonorId(leadDonorId);
	fileInfo->SetDupeLeadFailures(leadFailures);
	fileInfo->SetDupeLeadSwitches(leadSwitches);
	return rotated;
}

inline void DupeArticleFallback::RegisterLeadSuccess(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (articleInfo->GetDupeFallbackRound() != 1 ||
		articleInfo->GetDupeLeadSnapshot() == 0 ||
		articleInfo->GetDupeLeadSnapshot() != fileInfo->GetDupeLeadDonorId())
	{
		return;
	}
	int leadFailures = fileInfo->GetDupeLeadFailures();
	int leadSwitches = fileInfo->GetDupeLeadSwitches();
	nzbget_rs_dupe_fallback_register_lead_success(
		articleInfo->GetDupeFallbackRound(), articleInfo->GetDupeLeadSnapshot(),
		fileInfo->GetDupeLeadDonorId(),
		ExpectedSegmentOffset(fileInfo, articleInfo),
		ExpectedSegmentEnd(fileInfo, articleInfo),
		&leadFailures, &leadSwitches);
	fileInfo->SetDupeLeadFailures(leadFailures);
	fileInfo->SetDupeLeadSwitches(leadSwitches);
}

inline void DupeArticleFallback::VacateGhostLead(FileInfo* fileInfo, const RawNzbList& donors)
{
	// a lead gone from queue and history, or barred after its articles failed the
	// par2 check (B40), is vacated: a barred one was passed over when sources were
	// pinned, so no article's lead matched it again and the lead rotation froze
	int leadNzbId = fileInfo->GetDupeLeadDonorId();
	bool barred = fileInfo->GetNzbInfo() && fileInfo->GetNzbInfo()->GetDupeBlockedDonors()->count(leadNzbId);
	std::vector<int32_t> donorIds;
	donorIds.reserve(donors.size());
	for (NzbInfo* donor : donors)
	{
		donorIds.push_back(donor->GetId());
	}
	if (nzbget_rs_dupe_fallback_should_vacate_ghost_lead(
		leadNzbId, barred ? 1 : 0,
		donorIds.empty() ? nullptr : donorIds.data(),
		donorIds.size()) == 0)
	{
		return;
	}

	fileInfo->SetDupeLeadDonorId(0);
	fileInfo->SetDupeLeadFailures(0);
	fileInfo->SetDupeLeadSwitches(0);
}

inline CString DupeArticleFallback::NzbFilenameOf(FileInfo* fileInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	if (articles->empty())
	{
		return CString();
	}

	// the collection's own nzb-file is parsed (and cached) like a donor's
	NzbInfo* ownNzb = GetParsedDonor(fileInfo->GetNzbInfo());
	if (!ownNzb)
	{
		return CString();
	}

	// The own entry is found by an article message-id. Substituted articles
	// carry a duplicate's id (their original id is not kept across an
	// unload/reload of the article list), so any article whose id is still
	// the posting's own identifies the file.
	for (FileInfo* ownFile : ownNzb->GetFileList())
	{
		ArticleList* ownArticles = ownFile->GetArticles();
		if (ownArticles->size() != articles->size())
		{
			continue;
		}
		for (size_t i = 0; i < articles->size(); i++)
		{
			ArticleInfo* article = (*articles)[i].get();
			ArticleInfo* ownArticle = (*ownArticles)[i].get();
			if (nzbget_rs_dupe_fallback_own_article_matches(
				ownArticle->GetPartNumber(), article->GetPartNumber(),
				ownArticle->GetMessageId(),
				article->GetDupeOriginalMessageId(),
				article->GetMessageId()) != 0)
			{
				return CString(ownFile->GetFilename());
			}
		}
	}
	return CString();
}

inline RawNzbList DupeArticleFallback::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList donors;

	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		if (nzbget_rs_dupe_fallback_is_donor_eligible(
			queuedNzbInfo == nzbInfo ? 1 : 0,
			(int)queuedNzbInfo->GetKind(),
			(int)NzbInfo::nkNzb,
			(int)queuedNzbInfo->GetDupeMode(),
			(int)dmForce) != 0 &&
			nzbget_rs_dupecoord_same_name_or_key(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()) != 0)
		{
			donors.push_back(queuedNzbInfo);
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			nzbget_rs_dupe_fallback_is_donor_eligible(
				0,
				(int)historyInfo->GetKind(),
				(int)HistoryInfo::hkNzb,
				(int)historyInfo->GetNzbInfo()->GetDupeMode(),
				(int)dmForce) != 0 &&
			nzbget_rs_dupecoord_same_name_or_key(historyInfo->GetNzbInfo()->GetName(),
				historyInfo->GetNzbInfo()->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey()) != 0)
		{
			donors.push_back(historyInfo->GetNzbInfo());
		}
	}

	std::string ownIdentity = TwinCheck::Identity(nzbInfo);
	std::set<const NzbInfo*> twinDonors;
	if (!ownIdentity.empty())
	{
		for (NzbInfo* donor : donors)
		{
			if (nzbInfo->GetFullContentHash() > 0 &&
				nzbInfo->GetFullContentHash() == donor->GetFullContentHash())
			{
				continue;
			}
			std::string donorIdentity = TwinCheck::Identity(donor);
			std::string kind = !donorIdentity.empty() ?
				TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), donor->GetId(), donorIdentity.c_str()) : "";
			if (nzbget_rs_dupe_fallback_is_twin_kind(
				ownIdentity.c_str(), donorIdentity.c_str(), kind.c_str()) != 0)
			{
				twinDonors.insert(donor);
			}
		}
	}

	std::sort(donors.begin(), donors.end(),
		[&twinDonors](NzbInfo* donor1, NzbInfo* donor2)
		{
			bool twin1 = twinDonors.count(donor1) > 0;
			bool twin2 = twinDonors.count(donor2) > 0;
			return nzbget_rs_dupe_fallback_compare_donors(
				twin1 ? 1 : 0, donor1->GetDupeScore(), donor1->GetId(),
				twin2 ? 1 : 0, donor2->GetDupeScore(), donor2->GetId()) != 0;
		});

	return donors;
}

inline void DupeArticleFallback::PinSources(DownloadQueue* downloadQueue, FileInfo* fileInfo,
	ArticleInfo* articleInfo, bool twinOnly)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	RawNzbList donors = CollectDonors(downloadQueue, nzbInfo);
	VacateGhostLead(fileInfo, donors);
	if (twinOnly && fileInfo->GetDupeLeadDonorId() != 0)
	{
		std::string ownIdentity = TwinCheck::Identity(nzbInfo);
		int leadId = fileInfo->GetDupeLeadDonorId();
		auto it = std::find_if(donors.begin(), donors.end(),
			[leadId](NzbInfo* donor) { return donor->GetId() == leadId; });
		std::string leadIdentity = it != donors.end() ? TwinCheck::Identity(*it) : "";
		std::string leadKind = (!ownIdentity.empty() && !leadIdentity.empty()) ?
			TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), leadId, leadIdentity.c_str()) : "";
		if (nzbget_rs_dupe_fallback_should_reset_non_twin_lead(
			twinOnly ? 1 : 0, leadId, ownIdentity.c_str(), leadIdentity.c_str(), leadKind.c_str()) != 0)
		{
			fileInfo->SetDupeLeadDonorId(0);
			fileInfo->SetDupeLeadFailures(0);
			fileInfo->SetDupeLeadSwitches(0);
		}
	}
	RotateToLead(donors, fileInfo->GetDupeLeadDonorId());

	// the name the target's own nzb-file gives this file: the downloaded name
	// may have been replaced by an (obfuscated) name from the article itself,
	// while duplicate nzb-files still carry the subject names
	CString targetNzbFilename = donors.empty() ? CString() : NzbFilenameOf(fileInfo);

	std::vector<CString> candidates;
	std::vector<int> contributors;
	int samePosting = 0;
	int unparsed = 0;

	for (NzbInfo* donorNzbInfo : donors)
	{
		// one whose articles failed the par2 checksums of this download is out (B40)
		if (nzbInfo->GetDupeBlockedDonors()->count(donorNzbInfo->GetId()))
		{
			continue;
		}

		// an exact copy of the same posting shares the message-ids and cannot help
		if (nzbInfo->GetFullContentHash() > 0 &&
			nzbInfo->GetFullContentHash() == donorNzbInfo->GetFullContentHash())
		{
			samePosting++;
			continue;
		}

		// Extract this donor's candidate immediately: GetParsedDonor may evict a
		// previously-parsed donor from the bounded cache, so a parsed-donor pointer
		// must never be held across another GetParsedDonor call (use-after-free).
		NzbInfo* parsedDonor = GetParsedDonor(donorNzbInfo);
		if (parsedDonor)
		{
			// the donor's par2 fingerprint: its file list is matched by content (twin.rs)
			std::string donorIdentity = TwinCheck::Identity(donorNzbInfo);
			AppendDonorCandidate(candidates, contributors, donorNzbInfo->GetId(),
				parsedDonor, fileInfo, articleInfo->GetPartNumber(), targetNzbFilename,
				donorIdentity.c_str(), twinOnly);
		}
		else
		{
			unparsed++;
		}
	}

	if (!twinOnly && candidates.empty() && !donors.empty() && !fileInfo->GetDupeNoSourceLogged())
	{
		// the silent "did not even try" case: say why no duplicate could help,
		// once per file (it was once per missing article: dozens a second)
		fileInfo->SetDupeNoSourceLogged(true);
		nzbInfo->PrintMessage(Message::mkDetail,
			"No duplicate source for %s [%i]: %i duplicate(s), %i same posting, "
			"%i without readable nzb-file, none with a matching file (nzb name: %s); "
			"not repeated for its other articles",
			fileInfo->GetFilename(), articleInfo->GetPartNumber(), (int)donors.size(),
			samePosting, unparsed, targetNzbFilename.Empty() ? "unknown" : *targetNzbFilename);
	}

	FinishPin(fileInfo, articleInfo, candidates, contributors,
		fileInfo->GetDupeCutover(), articleInfo->GetDupeOriginalMessageId());
}

inline void DupeArticleFallback::AppendDonorCandidate(std::vector<CString>& candidates,
	std::vector<int>& contributors, int donorNzbId,
	NzbInfo* parsedDonor, FileInfo* targetFile, int partNumber, const char* targetNzbFilename,
	const char* donorPrint, bool twinOnly)
{
	FileInfo* donorFile = MatchDonorFile(targetFile, parsedDonor, targetNzbFilename,
		donorNzbId, donorPrint, twinOnly);
	if (!donorFile)
	{
		return;
	}

	const char* messageId = FindDonorMessageId(donorFile, partNumber);
	if (!messageId)
	{
		return;
	}

	bool duplicate = std::find_if(candidates.begin(), candidates.end(),
		[messageId](const CString& candidate) { return candidate == messageId; }) != candidates.end();
	if (!duplicate)
	{
		candidates.emplace_back(messageId);
		contributors.push_back(donorNzbId);
	}
}

inline std::unique_ptr<NzbInfo> DupeArticleFallback::ParseDonorNzb(const char* queuedFilename)
{
	if (Util::EmptyStr(queuedFilename) || !FileSystem::FileExists(queuedFilename))
	{
		return nullptr;
	}

	NzbFile nzbFile(queuedFilename, "");
	if (!nzbFile.Parse())
	{
		detail("Could not parse duplicate nzb-file %s", queuedFilename);
		return nullptr;
	}

	std::unique_ptr<NzbInfo> parsedNzb = nzbFile.DetachNzbInfo();

	if (g_Options->GetServerMode())
	{
		// in server mode the parser offloads article lists to disk-state and
		// clears them from memory; load them back (the donor is used in memory
		// only) and remove the disk-state files written as parse side effect
		for (FileInfo* fileInfo : parsedNzb->GetFileList())
		{
			g_DiskState->LoadArticles(fileInfo);
		}
		g_DiskState->DiscardFiles(parsedNzb.get(), false);
	}

	return parsedNzb;
}

inline NzbInfo* DupeArticleFallback::GetParsedDonor(NzbInfo* donorNzbInfo)
{
	int donorId = donorNzbInfo->GetId();

	// a duplicate whose nzb-file couldn't be read is asked again after a while: the
	// failure may have been passing (review item 9)
	auto bad = m_badDonors.find(donorId);
	if (bad != m_badDonors.end() && nzbget_rs_dupe_fallback_is_bad_donor_in_cooldown(
		(int64)Util::CurrentTime(), (int64)bad->second, BadDonorRetrySec) != 0)
	{
		return nullptr;
	}

	auto it = m_parsedDonors.find(donorId);
	if (it != m_parsedDonors.end())
	{
		return it->second.get();
	}

	// duplicates in history don't keep their article lists in memory; the
	// retained source nzb-file is parsed again instead (like "Download again")
	std::unique_ptr<NzbInfo> parsedNzb = ParseDonorNzb(donorNzbInfo->GetQueuedFilename());
	if (!parsedNzb)
	{
		m_badDonors[donorId] = Util::CurrentTime();
		return nullptr;
	}

	if ((int)m_parsedDonors.size() >= MaxCachedDonors)
	{
		m_parsedDonors.erase(m_parsedDonors.begin());
	}

	// store the parsed collection in the cache and return a pointer to the
	// owned object (the cache keeps it alive for the daemon's lifetime)
	std::unique_ptr<NzbInfo>& cachedDonor = m_parsedDonors[donorId];
	cachedDonor = std::move(parsedNzb);

	return cachedDonor.get();
}

inline FileInfo* DupeArticleFallback::MatchDonorFile(FileInfo* targetFile, NzbInfo* donorNzb,
	const char* targetNzbFilename, int donorNzbId, const char* donorPrint, bool twinOnly)
{
	if (IsParFile(targetFile))
	{
		return nullptr;
	}

	NzbInfo* own = targetFile->GetNzbInfo();
	std::string ownIdentity = own ? TwinCheck::Identity(own) : "";
	bool twinAllowed = false;
	if (twinOnly)
	{
		std::string kind = (donorNzbId && !ownIdentity.empty() && !Util::EmptyStr(donorPrint)) ?
			TwinCheck::Kind(own->GetId(), ownIdentity.c_str(), donorNzbId, donorPrint) : "";
		twinAllowed = donorNzbId && nzbget_rs_dupe_fallback_is_twin_kind(
			ownIdentity.c_str(), donorPrint, kind.c_str()) != 0;
		if (!twinAllowed)
		{
			return nullptr;
		}
	}
	std::string byContent = (donorNzbId && !ownIdentity.empty() && !Util::EmptyStr(donorPrint)) ?
		TwinCheck::MatchByContent(own->GetId(), ownIdentity.c_str(), targetFile->GetFilename(),
			targetNzbFilename, donorNzbId, donorPrint) : "";

	ArticleList* targetArticles = targetFile->GetArticles();
	std::vector<int32_t> targetParts(targetArticles->size());
	std::vector<int32_t> targetSizes(targetArticles->size());
	for (size_t i = 0; i < targetArticles->size(); i++)
	{
		targetParts[i] = (*targetArticles)[i]->GetPartNumber();
		targetSizes[i] = (*targetArticles)[i]->GetSize();
	}
	NzbgetRsDonorFileSpec targetSpec{};
	targetSpec.filename = targetFile->GetFilename();
	targetSpec.subject = targetFile->GetSubject();
	targetSpec.total_articles = targetFile->GetTotalArticles();
	targetSpec.size = targetFile->GetSize();
	targetSpec.is_par_file = 0;
	targetSpec.parts = targetParts.empty() ? nullptr : targetParts.data();
	targetSpec.sizes = targetSizes.empty() ? nullptr : targetSizes.data();
	targetSpec.article_count = targetParts.size();

	FileList* fileList = donorNzb->GetFileList();
	size_t n = fileList->size();
	std::vector<std::vector<int32_t>> donorPartsBuf(n);
	std::vector<std::vector<int32_t>> donorSizesBuf(n);
	std::vector<NzbgetRsDonorFileSpec> donorSpecs(n);
	for (size_t i = 0; i < n; i++)
	{
		FileInfo* df = (*fileList)[i].get();
		ArticleList* da = df->GetArticles();
		donorPartsBuf[i].resize(da->size());
		donorSizesBuf[i].resize(da->size());
		for (size_t j = 0; j < da->size(); j++)
		{
			donorPartsBuf[i][j] = (*da)[j]->GetPartNumber();
			donorSizesBuf[i][j] = (*da)[j]->GetSize();
		}
		donorSpecs[i].filename = df->GetFilename();
		donorSpecs[i].subject = df->GetSubject();
		donorSpecs[i].total_articles = df->GetTotalArticles();
		donorSpecs[i].size = df->GetSize();
		donorSpecs[i].is_par_file = IsParFile(df) ? 1 : 0;
		donorSpecs[i].parts = donorPartsBuf[i].empty() ? nullptr : donorPartsBuf[i].data();
		donorSpecs[i].sizes = donorSizesBuf[i].empty() ? nullptr : donorSizesBuf[i].data();
		donorSpecs[i].article_count = donorPartsBuf[i].size();
	}

	ptrdiff_t idx = nzbget_rs_dupe_fallback_match_donor_file(
		&targetSpec,
		targetNzbFilename,
		byContent.c_str(),
		twinAllowed ? 1 : 0,
		twinOnly ? 1 : 0,
		donorSpecs.empty() ? nullptr : donorSpecs.data(),
		donorSpecs.size());
	return (idx >= 0 && (size_t)idx < n) ? (*fileList)[(size_t)idx].get() : nullptr;
}

inline uint64 DupeArticleFallback::ArticleSizeStepsHash(FileInfo* fileInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	if ((int)articles->size() < MinStepFingerprintArticles)
	{
		return 0;
	}

	std::vector<int> sizes(articles->size());
	for (size_t i = 0; i < articles->size(); i++)
	{
		sizes[i] = (*articles)[i]->GetSize();
	}
	return nzbget_rs_dupe_fallback_steps_hash(sizes.data(), sizes.size());
}

inline const char* DupeArticleFallback::FindDonorMessageId(FileInfo* donorFile, int partNumber)
{
	for (ArticleInfo* article : donorFile->GetArticles())
	{
		if (article->GetPartNumber() == partNumber && !Util::EmptyStr(article->GetMessageId()))
		{
			return article->GetMessageId();
		}
	}

	return nullptr;
}

static inline int FindArticleIndex(ArticleList* articles, ArticleInfo* articleInfo)
{
	for (size_t i = 0; i < articles->size(); i++)
	{
		if ((*articles)[i].get() == articleInfo)
		{
			return (int)i;
		}
	}
	return -1;
}

inline int64 DupeArticleFallback::ExpectedSegmentOffset(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	int index = FindArticleIndex(articles, articleInfo);
	if (index < 0)
	{
		return -1;
	}

	ArticleInfo* prev = index > 0 ? (*articles)[index - 1].get() : nullptr;
	return nzbget_rs_dupe_fallback_expected_offset(
		index, articleInfo->GetPartNumber(),
		prev ? prev->GetPartNumber() : 0,
		prev ? (int)prev->GetStatus() : 0,
		prev ? prev->GetSegmentOffset() : 0,
		prev ? prev->GetSegmentSize() : 0);
}

inline int64 DupeArticleFallback::ExpectedSegmentEnd(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	int index = FindArticleIndex(articles, articleInfo);
	if (index < 0)
	{
		return -1;
	}

	ArticleInfo* next = (index + 1 < (int)articles->size()) ? (*articles)[index + 1].get() : nullptr;
	return nzbget_rs_dupe_fallback_expected_end(
		index, articles->size(), articleInfo->GetPartNumber(),
		fileInfo->GetDecodedFileSize(),
		next ? next->GetPartNumber() : 0,
		next ? (int)next->GetStatus() : 0,
		next ? next->GetSegmentOffset() : 0,
		next ? next->GetSegmentSize() : 0);
}

inline bool DupeArticleFallback::SegmentAligned(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	int64 begin = articleInfo->GetSegmentOffset();
	int size = articleInfo->GetSegmentSize();
	if (size <= 0)
	{
		return true;
	}
	return nzbget_rs_dupe_fallback_segment_aligned(
		begin, size,
		ExpectedSegmentOffset(fileInfo, articleInfo),
		ExpectedSegmentEnd(fileInfo, articleInfo)) != 0;
}

inline ArticleInfo* DupeArticleFallback::FirstUntiledArticle(FileInfo* fileInfo)
{
	// DecodedFileSize is only set from a decoded yEnc article; when it is 0 the
	// file is non-yEnc (e.g. uuencode, whose articles all record offset 0) or
	// its geometry is not yet known - either way the tiling cannot be judged.
	// After a restart it is repopulated from the first yEnc article that
	// completes, so it is available again by the time a reloaded file finishes.
	int64 decodedSize = fileInfo->GetDecodedFileSize();
	if (decodedSize <= 0)
	{
		return nullptr;
	}

	ArticleList* articles = fileInfo->GetArticles();
	std::vector<NzbgetRsArticleGeom> geom(articles->size());
	for (size_t i = 0; i < articles->size(); i++)
	{
		ArticleInfo* a = (*articles)[i].get();
		geom[i].part_number = a->GetPartNumber();
		geom[i].status = (int32_t)a->GetStatus();
		geom[i].segment_offset = a->GetSegmentOffset();
		geom[i].segment_size = a->GetSegmentSize();
		geom[i].fallback_round = a->GetDupeFallbackRound();
	}
	ptrdiff_t idx = nzbget_rs_dupe_fallback_first_untiled(
		geom.empty() ? nullptr : geom.data(), geom.size(), decodedSize);
	return (idx >= 0 && (size_t)idx < articles->size()) ? (*articles)[(size_t)idx].get() : nullptr;
}

inline bool DupeArticleFallback::HasDonorArticles(FileInfo* fileInfo)
{
	for (ArticleInfo* article : fileInfo->GetArticles())
	{
		if (article->GetDupeFallbackRound() > 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::MergeDecodedFileSize(FileInfo* fileInfo, int64 articleFileSize)
{
	int64 decodedFileSize = fileInfo->GetDecodedFileSize();
	bool conflict = nzbget_rs_dupe_fallback_merge_decoded_size(&decodedFileSize, articleFileSize) != 0;
	fileInfo->SetDecodedFileSize(decodedFileSize);
	return conflict;
}

inline FileInfo::EFirstContent DupeArticleFallback::FirstContent(FileInfo* fileInfo)
{
	if (fileInfo->GetFirstContent() != FileInfo::fcUnknown)
	{
		return fileInfo->GetFirstContent();
	}

	// after a restart: the first article was written already, its bytes tell
	ArticleList* articles = fileInfo->GetArticles();
	if (articles->empty() ||
		articles->front()->GetPartNumber() != 1 ||
		articles->front()->GetStatus() != ArticleInfo::aiFinished ||
		fileInfo->GetOutputFilename().empty())
	{
		return FileInfo::fcUnknown;
	}
	uint8 head[8] = {0};
	if (!nzbget_rs_fs_read_first_8_bytes(fileInfo->GetOutputFilename().c_str(), head))
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	int headKind = nzbget_rs_dupe_fallback_classify_first_head(head, sizeof(head));
	if (headKind == 0)
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	fileInfo->SetFirstContent(headKind == 1 ? FileInfo::fcPar2 : FileInfo::fcOther);
	return fileInfo->GetFirstContent();
}

#endif /* DUPEARTICLEFALLBACK_H */

#endif /* DISKSTATE_H */
