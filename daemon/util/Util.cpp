/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023-2026 Denis <denis@nzbget.com>
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


#include "nzbget.h"

#include <optional>
#include <sstream>
#include <array>
#include <algorithm>
#include "Util.h"
#include "nzbget_rs.h"
#include <climits>
#include <stdexcept>
#include <limits>

const char* Util::NULL_OUTPUT = " > /dev/null 2> /dev/null";
const char* Util::FIND_CMD = "which ";

char Util::VersionRevisionBuf[100];

void Util::Init()
{
	snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
	// init static vars there
	CurrentTicks();
}

/**
 * @brief Determines the appropriate program to execute a given file.
 *
 * @param filename    The name of the file to be executed.
 * @param customPath  A custom path for finding the executor.
 *
 * @return std::optional<std::string>
 * The path to the executor program if found,
 * std::nullopt otherwise.  If the file is directly
 * executable, the original filename is returned.
 */
std::optional<std::string>
Util::FindExecutorProgram(const std::string& filename, const std::string& customPath)
{
	size_t idx = 0;
	int extKind = nzbget_rs_dqh_util_classify_executor_ext(
		filename.data(), filename.size(), &idx);
	if (extKind == 0)
	{
		return filename;
	}

	const std::string fileExt = filename.substr(idx);

	auto res = FindShellOverriddenExecutor(fileExt, customPath);
	if (res.has_value())
	{
		return res;
	}

	if (extKind == 1)
	{
		return FindPython();
	}

	if (extKind == 2)
	{
		std::string cmd = std::string("bash --version") + NULL_OUTPUT;
		if (std::system(cmd.c_str()) == 0)
		{
			return std::string("bash");
		}
		cmd = std::string("sh --version") + NULL_OUTPUT;
		if (std::system(cmd.c_str()) == 0)
		{
			return std::string("sh");
		}
		return std::nullopt;
	}

	if (extKind == 3)
	{
		const std::string cmd = std::string("node --version") + NULL_OUTPUT;
		if (std::system(cmd.c_str()) == 0)
		{
			return std::string("node");
		}
		return std::nullopt;
	}

	return filename;
}

std::optional<std::string>
Util::FindShellOverriddenExecutor(const std::string& fileExt, const std::string& customPath)
{
	if (fileExt.empty() || customPath.empty())
	{
		return std::nullopt;
	}

	Tokenizer tok(customPath.c_str(), ",;");
	while (char* shellover = tok.Next())
	{
		size_t cmdOffset = 0;
		if (nzbget_rs_ufs_util_match_shell_overridden_token(
			shellover, fileExt.data(), fileExt.size(), &cmdOffset) != 0)
		{
			return std::string(shellover + cmdOffset);
		}
	}

	return std::nullopt;
}

std::optional<fs::path> Util::ResolvePathFromEnv(std::string_view path)
{
	const char* envPath = std::getenv("PATH");
	if (!envPath) return std::nullopt;
	NzbgetRsBuf buf = nzbget_rs_ufs_util_resolve_path_from_env(path.data(), path.size(), envPath);
	std::string resolved(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	if (resolved.empty()) return std::nullopt;
	return fs::path(resolved);
}

std::optional<std::string> Util::FindPython()
{
	std::array<std::string, 3> pythonExecutables{ "python3", "python", "py" }; 

	for (const auto& executable : pythonExecutables) 
	{
		std::string cmd = executable + " --version" + NULL_OUTPUT;
		if (std::system(cmd.c_str()) == 0) 
		{
			return executable;
		}
	}

	return std::nullopt;
}
 
int64 Util::JoinInt64(uint32 Hi, uint32 Lo)
{
	return nzbget_rs_ufs_util_join_int64(Hi, Lo);
}

void Util::SplitInt64(int64 Int64, uint32* Hi, uint32* Lo)
{
	nzbget_rs_ufs_util_split_int64(Int64, Hi, Lo);
}

CString Util::FormatSize(int64 fileSize)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_size(fileSize);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

CString Util::FormatSpeed(int64 bytesPerSecond)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_speed(bytesPerSecond);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

void Util::FormatTime(time_t timeSec, char* buffer, int bufsize)
{
#ifdef HAVE_CTIME_R_3
	ctime_r(&timeSec, buffer, bufsize);
#else
	ctime_r(&timeSec, buffer);
#endif
	buffer[bufsize-1] = '\0';

	// trim LF
	buffer[strlen(buffer) - 1] = '\0';
}

CString Util::FormatTime(time_t timeSec)
{
	CString result;
	result.Reserve(50);
	FormatTime(timeSec, result, 50);
	return result;
}

CString Util::FormatBuffer(const char* buf, int len)
{
	// rust/src/util.rs
	NzbgetRsBuf out = nzbget_rs_format_buffer(buf, len);
	CString result(out.data, static_cast<int>(out.len));
	nzbget_rs_free(out);
	return result;
}

namespace
{
	// tolower as strcasecmp applies it (an unsigned byte), and as WildMask's
	// C++ code did (a char, signed or not)
	int FileExtCaseFold(int byte)
	{
		return tolower(byte);
	}

	int FileExtMaskFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return tolower(ch);
#else
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

bool Util::MatchFileExt(const char* filename, const char* extensionList, const char* listSeparator)
{
	// rust/src/util.rs
#ifdef __GLIBC__
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	return nzbget_rs_match_file_ext(filename, extensionList, listSeparator, table, CHAR_MIN < 0,
		FileExtCaseFold, FileExtMaskFold) != 0;
}

std::vector<CString> Util::SplitCommandLine(const char* commandLine)
{
	// rust/src/util.rs: the words back to back, each NUL-terminated
	NzbgetRsBuf words = nzbget_rs_split_command_line(commandLine);
	std::vector<CString> result;
	try
	{
		for (size_t i = 0; i < words.len; i += strlen(words.data + i) + 1)
		{
			result.emplace_back(words.data + i);
		}
	}
	catch (...)
	{
		nzbget_rs_free(words);
		throw;
	}
	nzbget_rs_free(words);
	return result;
}

void Util::TrimRight(char* str)
{
	// rust/src/util.rs
	nzbget_rs_trim_line(str, 1);
}

namespace
{
	int TextRightSpace(int byte)
	{
		// The caller can override char's signedness independently of Rust.
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		// glibc supports negative signed chars, keeping EOF distinct from 255.
		return std::isspace(ch);
#else
		// Negative chars other than EOF are outside the portable ctype domain.
		return ch < 0 ? 0 : std::isspace(ch);
#endif
	}
}

void Util::TrimRight(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 0, 1, 0, &start, TextRightSpace));
}

void Util::TrimLeft(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	nzbget_rs_trim_string(str.data(), str.size(), 1, 0, 0, &start, TextRightSpace);
	str.erase(0, start);
}

char* Util::Trim(char* str)
{
	// rust/src/util.rs
	return nzbget_rs_trim_line(str, 0);
}

void Util::Trim(std::string& str)
{
	TrimLeft(str);
	TrimRight(str);
}

void Util::SanitizeLine(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 1, 1, 1, &start, TextRightSpace));
	str.erase(0, start);
}

char* Util::ReduceStr(char* str, const char* from, const char* to)
{
	// rust/src/util.rs
	nzbget_rs_reduce_str(str, from, to);
	return str;
}

std::vector<CString> Util::SplitStr(const char* str, const char* separators)
{
	std::vector<CString> result;
	Tokenizer tok(str, separators);
	while (const char* substr = tok.Next())
	{
		result.emplace_back(substr);
	}
	return result;
}

bool Util::EndsWith(std::string_view str, std::string_view suffix, bool caseSensitive)
{
	// rust/src/util.rs
	return nzbget_rs_ends_with(str.data(), str.size(), suffix.data(), suffix.size(), caseSensitive) != 0;
}

bool Util::EndsWith(const char* str, const char* suffix, bool caseSensitive)
{
	if (!str)
	{
		return false;
	}
	if (EmptyStr(suffix))
	{
		return true;
	}
	return EndsWith(std::string_view(str), std::string_view(suffix), caseSensitive);
}

bool Util::AlphaNum(const char* str)
{
	// rust/src/util.rs
	return nzbget_rs_alpha_num(str) != 0;
}

uint32 Util::HashBJ96(const char* buffer, int bufSize, uint32 initValue)
{
	// rust/src/util.rs
	return nzbget_rs_hash_bj96(buffer, bufSize, initValue);
}

std::unique_ptr<FILE, std::function<void(FILE*)>> Util::MakePipe(const std::string& cmd)
{
	FILE* pipe = popen(cmd.c_str(), "r");

	return std::unique_ptr<FILE, std::function<void(FILE*)>>(pipe, [](FILE* pipe)
		{
			if (pipe)
			{
				std::ignore = pclose(pipe);
			}
		}
	);
}

time_t Util::CurrentTime()
{
	return ::time(nullptr);
}

time_t Util::Timegm(tm const *t)
{
	return (time_t)nzbget_rs_ufs_util_internal_timegm(
		t->tm_year, t->tm_mon, t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec);
}

bool Util::StrCaseCmp(std::string_view a, std::string_view b)
{
	return nzbget_rs_dqh_slice_eq_ignore_case(
		a.data(), a.size(), b.data(), b.size()) != 0;
}

// prevent PC from going to sleep
void Util::SetStandByMode([[maybe_unused]] bool standBy)
{
}

int Util::NumberOfCpuCores()
{
	return std::thread::hardware_concurrency();
}

int64 Util::CurrentTicks()
{
	timeval t;
	gettimeofday(&t, nullptr);
	return (int64)(t.tv_sec) * 1000000ll + (int64)(t.tv_usec);
}

void Util::Sleep(int ms)
{
	std::this_thread::sleep_for(std::chrono::milliseconds(ms));
}

uint32 WebUtil::DecodeBase64(char* inputBuffer, int inputBufferLength, char* outputBuffer)
{
	// rust/src/decode.rs
	return nzbget_rs_decode_base64(inputBuffer, inputBufferLength, outputBuffer);
}

/* END - Base64
*/

CString WebUtil::XmlEncode(const char* raw)
{
	// rust/src/escape.rs
	NzbgetRsBuf buf = nzbget_rs_xml_encode(raw);
	// CString adds one to an int length before allocating.
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

namespace
{
	// Numeric XML references use the caller's locale-specific case mapping.
	int XmlDigitLower(int byte)
	{
		return tolower(byte);
	}
}

void WebUtil::XmlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_decode(raw, XmlDigitLower);
}

const char* WebUtil::XmlFindTag(const char* xml, const char* tag, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_xml_find_tag(xml, tag, valueLength);
}

bool WebUtil::XmlParseTagValue(const char* xml, const char* tag, char* valueBuf, int valueBufSize, const char** tagEnd)
{
	int valueLen = 0;
	const char* value = XmlFindTag(xml, tag, &valueLen);
	if (!value)
	{
		return false;
	}
	int len = nzbget_rs_ufs_webutil_xml_calc_copy_len(valueLen, valueBufSize);
	strncpy(valueBuf, value, len);
	valueBuf[len] = '\0';
	if (tagEnd)
	{
		*tagEnd = value + valueLen;
	}
	return true;
}

void WebUtil::XmlStripTags(char* xml)
{
	// rust/src/text.rs
	nzbget_rs_xml_strip_tags(xml);
}

namespace
{
	// Classify the byte as the caller's char, preserving glibc's legacy
	// negative-byte entries (in particular, EOF is distinct from byte 255).
	int XmlEntityAlpha(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return (*__ctype_b_loc())[ch] & _ISalpha;
#else
		// Darwin and musl do not classify negative chars as alphabetic.
		// Avoid passing them outside the standard isalpha domain.
		return ch < 0 ? 0 : isalpha(ch);
#endif
	}
}

void WebUtil::XmlRemoveEntities(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_remove_entities(raw, XmlEntityAlpha);
}

CString WebUtil::JsonEncode(const char* raw)
{
	// rust/src/escape.rs
	NzbgetRsBuf buf = nzbget_rs_json_encode(raw);
	// CString adds one to an int length before allocating.
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

void WebUtil::JsonDecode(char* raw)
{
	// rust/src/decode.rs
	nzbget_rs_json_decode(raw);
}

const char* WebUtil::JsonFindField(const char* jsonText, const char* fieldName, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_json_find_field(jsonText, fieldName, valueLength);
}

const char* WebUtil::JsonNextValue(const char* jsonText, int* valueLength)
{
	// rust/src/decode.rs
	return nzbget_rs_json_next_value(jsonText, valueLength);
}

void WebUtil::HttpUnquote(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_http_unquote(raw);
}

namespace
{
	// tolower as strncasecmp applies it: to an unsigned byte
	int CaseFold(int byte)
	{
		return tolower(byte);
	}
}

CString WebUtil::ParseContentDispositionFilename(const char* contentDisposition)
{
	// rust/src/webutil.rs
#ifdef __GLIBC__
	// what strncasecmp reads: the tolower table of this thread's locale
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	NzbgetRsBuf buf = nzbget_rs_content_disposition_filename(contentDisposition, table, CaseFold);
	if (!buf.data)
	{
		return CString();
	}
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

void WebUtil::UrlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_url_decode(raw);
}

CString WebUtil::UrlEncode(const char* raw)
{
	// rust/src/text.rs
	NzbgetRsBuf buf = nzbget_rs_url_encode(raw);
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

namespace
{
	// Caller-owned bounded encodings; Reserve allocates bound+1 bytes and
	// the final NUL supplies CString's logical Length().
	template <class Write>
	CString RustEncodedKnownResult(size_t inputLen, size_t expansion, Write write)
	{
		if (inputLen > (static_cast<size_t>(std::numeric_limits<int>::max()) - 1) / expansion)
		{
			std::abort();
		}
		size_t bound = inputLen * expansion;
		CString result;
		result.Reserve(static_cast<int>(bound));
		char* output = static_cast<char*>(result);
		if (!output || write(output, bound + 1) > bound)
		{
			// Includes SIZE_MAX rejection; never request an unbounded retry.
			std::abort();
		}
		return result;
	}

}

CString WebUtil::Latin1ToUtf8(const char* str)
{
	// rust/src/text.rs
	size_t inputLen = str ? strlen(str) : 0;
	return RustEncodedKnownResult(inputLen, 2, [str, inputLen](char* out, size_t cap) {
		return nzbget_rs_latin1_to_utf8_n(str, inputLen, out, cap);
	});
}

/*
 The date/time can be formatted according to RFC822 in different ways. Examples:
   Wed, 26 Jun 2013 01:02:54 -0600
   Wed, 26 Jun 2013 01:02:54 GMT
   26 Jun 2013 01:02:54 -0600
   26 Jun 2013 01:02 -0600
   26 Jun 2013 01:02 A
 The day name and the seconds are optional; the zone is an offset (+hhmm or
 -hhmm) or a name (UT, GMT, UTC, Z and the US zones). Military zones and
 unknown names count as UTC. Only the first format was read before: the others
 (GMT is common in feeds) gave no time at all.
*/
time_t WebUtil::ParseRfc822DateTime(const char* dateTimeStr)
{
	// rust/src/util.rs
	return static_cast<time_t>(nzbget_rs_parse_rfc822_date_time(dateTimeStr));
}


URL::URL(const char* address) :
	m_address(address)
{
	if (address)
	{
		ParseUrl();
	}
}

void URL::ParseUrl()
{
	// rust/src/url.rs
	NzbgetRsUrlParts r;
	nzbget_rs_parse_url(m_address, &r);
	if (!r.valid)
	{
		return;
	}
	CString* fields[] = {&m_protocol, &m_user, &m_password, &m_host, &m_resource};
	for (int i = 0; i < 5; i++)
	{
		if (r.parts[i][0] >= 0)
		{
			// a part of the address; an empty one (only the host can be) as ""
			fields[i]->Set(r.parts[i][1] > 0 ? m_address + r.parts[i][0] : "", static_cast<int>(r.parts[i][1]));
		}
	}
	if (r.parts[4][0] < 0)
	{
		m_resource = "/";
	}
	m_port = r.port;
	m_valid = true;
}


RegEx::RegEx(const char *pattern, int matchBufSize) :
	m_matchBufSize(matchBufSize)
{
#ifdef HAVE_REGEX_H
	m_valid = regcomp(&m_context, pattern, REG_EXTENDED | REG_ICASE | (matchBufSize > 0 ? 0 : REG_NOSUB)) == 0;
	if (matchBufSize > 0)
	{
		m_matches = std::make_unique<regmatch_t[]>(matchBufSize);
	}
	else
	{
		m_matches = nullptr;
	}
#else
	m_valid = false;
#endif
}

RegEx::~RegEx()
{
#ifdef HAVE_REGEX_H
	regfree(&m_context);
#endif
}

bool RegEx::Match(const char *str)
{
#ifdef HAVE_REGEX_H
	return m_valid ? regexec(&m_context, str, m_matchBufSize, m_matches.get(), 0) == 0 : false;
#else
	return false;
#endif
}

int RegEx::GetMatchCount()
{
#ifdef HAVE_REGEX_H
	int count = 0;
	if (m_matches)
	{
		while (count < m_matchBufSize && m_matches[count].rm_so > -1)
		{
			count++;
		}
	}
	return count;
#else
	return 0;
#endif
}

int RegEx::GetMatchStart(int index)
{
#ifdef HAVE_REGEX_H
	return m_matches[index].rm_so;
#else
	return 0;
#endif
}

int RegEx::GetMatchLen(int index)
{
#ifdef HAVE_REGEX_H
	return nzbget_rs_ufs_regex_calc_match_len(m_matches[index].rm_so, m_matches[index].rm_eo);
#else
	return 0;
#endif
}


namespace
{
	// tolower of a byte as WildMask's C++ code saw it: a char (signed or not, as
	// on the platform) in the current locale, EOF kept distinct from 255
	int WildMaskFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		// glibc supports negative signed chars for legacy programs
		return tolower(ch);
#else
		// Darwin and musl leave negative characters unchanged; don't pass them
		// outside the standard tolower domain
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

bool WildMask::Match(const char* text)
{
	// rust/src/wildmask.rs
#ifdef __GLIBC__
	// what tolower reads: the table of this thread's locale
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	m_wildCount = 0;
	m_wildStart.clear();
	m_wildLen.clear();
	if (!m_wantsPositions)
	{
		return nzbget_rs_wild_match(m_pattern, text, nullptr, 0, table, CHAR_MIN < 0, WildMaskFold).matched != 0;
	}

	// Backtracking can retain more captures than there are pattern bytes.
	// Rust reports the total count even for failed or truncated matches.
	size_t capacity = 100;
	std::unique_ptr<int[][2]> positions(new int[capacity][2]);
	auto match = [&]() {
		return nzbget_rs_wild_match(m_pattern, text,
			positions.get(), capacity, table, CHAR_MIN < 0, WildMaskFold);
	};
	auto result = match();
	if (result.count > capacity)
	{
		capacity = result.count;
		positions.reset(new int[capacity][2]);
		result = match();
	}
	if (result.count > static_cast<size_t>(INT_MAX))
	{
		throw std::length_error("Too many wildcard captures");
	}
	m_wildCount = static_cast<int>(result.count);
	m_wildStart.resize(m_wildCount);
	m_wildLen.resize(m_wildCount);
	for (int i = 0; i < m_wildCount; i++)
	{
		m_wildStart[i] = positions[i][0];
		m_wildLen[i] = positions[i][1];
	}
	return result.matched != 0;
}


#ifndef DISABLE_GZIP
uint32 ZLib::GZipLen(int inputBufferLength)
{
	z_stream zstr{};
	return (uint32)deflateBound(&zstr, inputBufferLength);
}

uint32 ZLib::GZip(const void* inputBuffer, int inputBufferLength, void* outputBuffer, int outputBufferLength)
{
	z_stream zstr;
	zstr.zalloc = Z_NULL;
	zstr.zfree = Z_NULL;
	zstr.opaque = Z_NULL;
	zstr.next_in = (Bytef*)inputBuffer;
	zstr.avail_in = inputBufferLength;
	zstr.next_out = (Bytef*)outputBuffer;
	zstr.avail_out = outputBufferLength;

	/* add 16 to MAX_WBITS to enforce gzip format */
	if (Z_OK != deflateInit2(&zstr, Z_DEFAULT_COMPRESSION, Z_DEFLATED, MAX_WBITS + 16, MAX_MEM_LEVEL, Z_DEFAULT_STRATEGY))
	{
		return 0;
	}

	uint32 total_out = 0;
	if (deflate(&zstr, Z_FINISH) == Z_STREAM_END)
	{
		total_out = (uint32)zstr.total_out;
	}

	deflateEnd(&zstr);

	return total_out;
}

GUnzipStream::GUnzipStream(int BufferSize)
	: m_bufferSize(BufferSize)
	, m_outputBuffer(std::make_unique<Bytef[]>(BufferSize))
{

	/* add 16 to MAX_WBITS to enforce gzip format */
	int ret = inflateInit2(&m_zStream, MAX_WBITS + 16);
	m_active = ret == Z_OK;
}

GUnzipStream::~GUnzipStream()
{
	if (m_active)
	{
		inflateEnd(&m_zStream);
	}
}

void GUnzipStream::Write(const void *inputBuffer, int inputBufferLength)
{
	m_zStream.next_in = (Bytef*)inputBuffer;
	m_zStream.avail_in = inputBufferLength;
}

GUnzipStream::EStatus GUnzipStream::Read(const void **outputBuffer, int *outputBufferLength)
{
	m_zStream.next_out = (Bytef*)m_outputBuffer.get();
	m_zStream.avail_out = m_bufferSize;

	*outputBufferLength = 0;

	if (!m_active)
	{
		return zlError;
	}

	int ret = inflate(&m_zStream, Z_NO_FLUSH);

	switch (ret)
	{
		case Z_STREAM_END:
		case Z_OK:
			*outputBufferLength = m_bufferSize - m_zStream.avail_out;
			*outputBuffer = m_outputBuffer.get();
			return ret == Z_STREAM_END ? zlFinished : zlOK;

		case Z_BUF_ERROR:
			return zlOK;
	}

	return zlError;
}
#endif

Tokenizer::Tokenizer(const char* dataString, const char* separators) :
	m_separators(separators)
{
	// an optimization to avoid memory allocation for short data string
	int len = strlen(dataString);
	if (nzbget_rs_ufs_tokenizer_fits_short_string(len, m_shortString.Capacity()) != 0)
	{
		m_shortString.Set(dataString);
		m_dataString = m_shortString;
	}
	else
	{
		m_longString.Set(dataString);
		m_dataString = m_longString;
	}

}

Tokenizer::Tokenizer(char* dataString, const char* separators, bool inplaceBuf) :
	m_separators(separators)
{
	if (inplaceBuf)
	{
		m_dataString = dataString;
	}
	else
	{
		m_longString.Set(dataString);
		m_dataString = m_longString;
	}
}

char* Tokenizer::Next()
{
	char* token = nullptr;
	while (!token || !*token)
	{
		token = strtok_r(m_working ? nullptr : m_dataString, m_separators, &m_savePtr);
		m_working = true;
		if (!token)
		{
			return nullptr;
		}
		token = Util::Trim(token);
	}
	return token;
}

void Crc32::Reset()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = 0;
}

void Crc32::Append(uchar* block, uint32 length)
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = rapidyenc_crc(block, length, *currentCrc);
}

uint32 Crc32::Finish()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	return *currentCrc;
}

uint32 Crc32::Combine(uint32 crc1, uint32 crc2, uint32 len2)
{
	// rust/src/crc.rs
	return nzbget_rs_crc32_combine(crc1, crc2, len2);
}
