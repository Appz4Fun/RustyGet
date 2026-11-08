/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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


#ifndef UTIL_H

#include <optional>
#include <type_traits>
#include <string_view>

#ifndef UTIL_H
#define UTIL_H

class Util
{
public:
	static bool MatchFileExt(const char* filename, const char* extensionList, const char* listSeparator);
	static std::optional<std::string> 
	FindExecutorProgram(const std::string& filename, const std::string& customPath);

	/*
	* Split command line into arguments.
	* Uses spaces and single quotation marks as separators.
	* May return empty list if bad escaping was detected.
	*/
	static std::vector<CString> SplitCommandLine(const char* commandLine);

	static int64 JoinInt64(uint32 Hi, uint32 Lo);
	static void SplitInt64(int64 Int64, uint32* Hi, uint32* Lo);

	static std::optional<double> StrToFloatChecked(std::string_view str, double minVal, double maxVal);
	static std::optional<int64_t> StrToSignedChecked(std::string_view str, int64_t minVal, int64_t maxVal);
	static std::optional<uint64_t> StrToUnsignedChecked(std::string_view str, uint64_t maxVal);
	static int64_t SafeCastSignedToSigned(int64_t num, int64_t minTo, int64_t maxTo) noexcept;
	static int64_t SafeCastUnsignedToSigned(uint64_t num, uint64_t maxTo) noexcept;
	static uint64_t SafeCastSignedToUnsigned(int64_t num, uint64_t maxTo) noexcept;
	static uint64_t SafeCastUnsignedToUnsigned(uint64_t num, uint64_t maxTo) noexcept;

	template <typename T>
	static std::optional<T> StrToNum(std::string_view str)
	{
		static_assert(std::is_arithmetic_v<T>, "T must be an arithmetic type");

		if constexpr (std::is_floating_point_v<T>)
		{
			auto v = StrToFloatChecked(
				str,
				static_cast<double>(std::numeric_limits<T>::lowest()),
				static_cast<double>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			auto v = StrToSignedChecked(
				str,
				static_cast<int64_t>(std::numeric_limits<T>::min()),
				static_cast<int64_t>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
		else
		{
			auto v = StrToUnsignedChecked(
				str,
				static_cast<uint64_t>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
	}

	static void TrimRight(char* str);
	static void TrimRight(std::string& str);
	static void TrimLeft(std::string& str);
	static char* Trim(char* str);
	static void Trim(std::string& str);
	static bool IsControlChar(char c) noexcept;
	static void SanitizeLine(std::string& str);
	static bool EmptyStr(const char* str) { return !str || !*str; }
	static std::vector<CString> SplitStr(const char* str, const char* separators);
	static bool EndsWith(std::string_view str, std::string_view suffix, bool caseSensitive);
	static bool EndsWith(const char* str, const char* suffix, bool caseSensitive);
	static bool AlphaNum(const char* str);

	template<typename T, typename U>
	static constexpr bool CmpEqual(T t, U u) noexcept
	{
		if constexpr (std::is_signed_v<T> == std::is_signed_v<U>)
		{
			return t == u;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			return t >= 0 && std::make_unsigned_t<T>(t) == u;
		}
		else
		{
			return u >= 0 && std::make_unsigned_t<U>(u) == t;
		}
	}
	
	template<typename T, typename U>
	static constexpr bool CmpNotEqual(T t, U u) noexcept
	{
		return !CmpEqual(t, u);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpLess(T t, U u) noexcept
	{
		if constexpr (std::is_signed_v<T> == std::is_signed_v<U>)
		{
			return t < u;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			return t < 0 || std::make_unsigned_t<T>(t) < u;
		}
		else
		{
			return u >= 0 && t < std::make_unsigned_t<U>(u);
		}
	}
	
	template<typename T, typename U>
	static constexpr bool CmpGreater(T t, U u) noexcept
	{
		return CmpLess(u, t);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpLessEqual(T t, U u) noexcept
	{
		return !CmpLess(u, t);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpGreaterEqual(T t, U u) noexcept
	{
		return !CmpLess(t, u);
	}
	
	template <typename From, typename To,
	 	typename std::enable_if_t<std::is_integral_v<From> && std::is_integral_v<To>, bool> = true>
	static To SafeIntCast(From num) noexcept
	{
		if constexpr (std::is_unsigned_v<From> && std::is_signed_v<To>)
		{
			return static_cast<To>(SafeCastUnsignedToSigned(
				static_cast<uint64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
		else if constexpr (std::is_signed_v<From> && std::is_unsigned_v<To>)
		{
			return static_cast<To>(SafeCastSignedToUnsigned(
				static_cast<int64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
		else if constexpr (std::is_signed_v<From> && std::is_signed_v<To>)
		{
			return static_cast<To>(SafeCastSignedToSigned(
				static_cast<int64_t>(num),
				static_cast<int64_t>(std::numeric_limits<To>::min()),
				static_cast<int64_t>(std::numeric_limits<To>::max())));
		}
		else
		{
			return static_cast<To>(SafeCastUnsignedToUnsigned(
				static_cast<uint64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
	}
	

	/* replace all occurences of szFrom to szTo in string szStr with a limitation that szTo must be shorter than szFrom */
	static char* ReduceStr(char* str, const char* from, const char* to);

	/* Calculate Hash using Bob Jenkins (1996) algorithm */
	static uint32 HashBJ96(const char* buffer, int bufSize, uint32 initValue);

	static void SetStandByMode(bool standBy);

	static time_t CurrentTime();
	static int64 CurrentTicks();
	static void Sleep(int milliseconds);

	/* cross platform version of GNU timegm, which is similar to mktime but takes an UTC time as parameter */
	static time_t Timegm(tm const *t);

	static bool StrCaseCmp(std::string_view a, std::string_view b);

	static void FormatTime(time_t timeSec, char* buffer, int bufsize);
	static CString FormatTime(time_t timeSec);

	static CString FormatSpeed(int64 bytesPerSecond);
	static CString FormatSize(int64 fileSize);
	static CString FormatBuffer(const char* buf, int len);

	/*
	* Returns program version and revision number as string formatted like "0.7.0-r295".
	* If revision number is not available only version is returned ("0.7.0").
	*/
	static const char* VersionRevision() { return VersionRevisionBuf; }

	static char VersionRevisionBuf[100];

	static void Init();

	/*
	* Returns number of available CPU cores or -1 if it could not be determined
	*/
	static int NumberOfCpuCores();
};

class WebUtil
{
public:
	static uint32 DecodeBase64(char* inputBuffer, int inputBufferLength, char* outputBuffer);

	/*
	* Encodes string to be used as content of xml-tag.
	*/
	static CString XmlEncode(const char* raw);

	/*
	* Decodes string from xml.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void XmlDecode(char* raw);

	/*
	* Returns pointer to tag-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* XmlFindTag(const char* xml, const char* tag, int* valueLength);

	/*
	* Parses tag-content into szValueBuf.
	*/
	static bool XmlParseTagValue(const char* xml, const char* tag, char* valueBuf, int valueBufSize, const char** tagEnd);

	/*
	* Replaces all tags with spaces effectively providing the text content only.
	* The string is transformed in-place overwriting the previous content.
	*/
	static void XmlStripTags(char* xml);

	/*
	* Replaces all entities with spaces.
	* The string is transformed in-place overwriting the previous content.
	*/
	static void XmlRemoveEntities(char* raw);

	/*
	* Creates JSON-string by replace the certain characters with escape-sequences.
	*/
	static CString JsonEncode(const char* raw);

	/*
	* Decodes JSON-string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void JsonDecode(char* raw);

	/*
	* Returns pointer to field-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* JsonFindField(const char* jsonText, const char* fieldName, int* valueLength);

	/*
	* Returns pointer to field-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* JsonNextValue(const char* jsonText, int* valueLength);

	/*
	* Unquote http quoted string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void HttpUnquote(char* raw);

	/*
	* Extracts file name from the value of "Content-Disposition" HTTP header (RFC 6266).
	* The extended parameter "filename*" (RFC 5987) takes precedence over "filename".
	* Returns an empty string if the header doesn't contain a file name.
	*/
	static CString ParseContentDispositionFilename(const char* contentDisposition);

	/*
	* Decodes URL-string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void UrlDecode(char* raw);

	/*
	* Makes valid URL by replacing of spaces with "%20".
	*/
	static CString UrlEncode(const char* raw);

	/*
	* Converts ISO-8859-1 (aka Latin-1) into UTF-8.
	*/
	static CString Latin1ToUtf8(const char* str);

	static time_t ParseRfc822DateTime(const char* dateTimeStr);
};

class URL
{
public:
	URL(const char* address);
	bool IsValid() { return m_valid; }
	const char* GetAddress() { return m_address; }
	const char* GetProtocol() { return m_protocol; }
	const char* GetUser() { return m_user; }
	const char* GetPassword() { return m_password; }
	const char* GetHost() { return m_host; }
	const char* GetResource() { return m_resource; }
	int GetPort() { return m_port; }
	bool GetTls() { return m_tls; }

private:
	CString m_address;
	CString m_protocol;
	CString m_user;
	CString m_password;
	CString m_host;
	CString m_resource;
	int m_port = 0;
	bool m_tls = false;
	bool m_valid = false;

	void ParseUrl();
};

class RegEx
{
public:
	RegEx(const char *pattern, int matchBufSize = 100);
	~RegEx();
	bool IsValid();
	bool Match(const char* str);
	int GetMatchCount();
	int GetMatchStart(int index);
	int GetMatchLen(int index);

private:
#ifdef HAVE_REGEX_H
	regex_t m_context;
#endif
	void* m_rsRegex = nullptr;
};

class WildMask
{
public:
	WildMask(const char* pattern, bool wantsPositions = false):
		m_pattern(pattern), m_wantsPositions(wantsPositions), m_wildCount(0) {}
	bool Match(const char* text);
	int GetMatchCount() { return m_wildCount; }
	int GetMatchStart(int index) { return m_wildStart[index]; }
	int GetMatchLen(int index) { return m_wildLen[index]; }

private:
	typedef std::vector<int> IntList;

	CString m_pattern;
	bool m_wantsPositions;
	int m_wildCount;
	IntList m_wildStart;
	IntList m_wildLen;

	void ExpandArray();
};

#ifndef DISABLE_GZIP
class ZLib
{
public:
	/*
	* calculates the size required for output buffer
	*/
	static uint32 GZipLen(int inputBufferLength);

	/*
	* compresses inputBuffer and returns the size of bytes written to
	* outputBuffer or 0 if the buffer is too small or an error occured.
	*/
	static uint32 GZip(const void* inputBuffer, int inputBufferLength, void* outputBuffer, int outputBufferLength);
};

class GUnzipStream
{
public:
	enum EStatus
	{
		zlError,
		zlFinished,
		zlOK
	};

	GUnzipStream(int BufferSize);
	~GUnzipStream();

	/*
	* set next memory block for uncompression
	*/
	void Write(const void *inputBuffer, int inputBufferLength);

	/*
	* get next uncompressed memory block.
	* iOutputBufferLength - the size of uncompressed block. if it is "0" the next compressed block must be provided via "Write".
	*/
	EStatus Read(const void **outputBuffer, int *outputBufferLength);

private:
	void* m_rsStream = nullptr;
};
#endif

class Tokenizer
{
public:
	Tokenizer(const char* dataString, const char* separators);
	Tokenizer(char* dataString, const char* separators, bool inplaceBuf);
	char* Next();

private:
	BString<1024> m_shortString;
	CString m_longString;
	char* m_dataString;
	const char* m_separators;
	char* m_savePtr = nullptr;
	bool m_working = false;
};

class Crc32
{
public:
	Crc32() { Reset(); }
	void Reset();
	void Append(uchar* block, uint32 length);
	uint32 Finish();
	static uint32 Combine(uint32 crc1, uint32 crc2, uint32 len2);

private:
	alignas(16) uint32_t m_state[4 * 5]; // = YEncode::crc_state
	void* State() { return &m_state; }
};

#include <optional>
#include <sstream>
#include <array>
#include <algorithm>
#include "nzbget_rs.h"
#include <climits>
#include <stdexcept>
#include <limits>
#include <memory>

// Keep the Rust allocation owned even if std::string's allocation throws.
static inline std::string CopyRustPathText(NzbgetRsBuf buf)
{
	const auto release = [buf](char*) { nzbget_rs_free(buf); };
	const std::unique_ptr<char, decltype(release)> owner(buf.data, release);
	return std::string(buf.data, buf.len);
}

inline char Util::VersionRevisionBuf[100];

inline void Util::Init()
{
	snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
	// init static vars there
	CurrentTicks();
}

inline std::optional<std::string>
Util::FindExecutorProgram(const std::string& filename, const std::string& customPath)
{
	NzbgetRsBuf buf{};
	if (nzbget_rs_util_find_executor_program(
		filename.data(), filename.size(), customPath.data(), customPath.size(), &buf) == 0)
	{
		nzbget_rs_free(buf);
		return std::nullopt;
	}
	return CopyRustPathText(buf);
}

inline int64 Util::JoinInt64(uint32 Hi, uint32 Lo)
{
	return nzbget_rs_ufs_util_join_int64(Hi, Lo);
}

inline void Util::SplitInt64(int64 Int64, uint32* Hi, uint32* Lo)
{
	nzbget_rs_ufs_util_split_int64(Int64, Hi, Lo);
}

inline std::optional<double> Util::StrToFloatChecked(std::string_view str, double minVal, double maxVal)
{
	double out = 0.0;
	if (nzbget_rs_util_str_to_f64_checked(str.data(), str.size(), minVal, maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline std::optional<int64_t> Util::StrToSignedChecked(std::string_view str, int64_t minVal, int64_t maxVal)
{
	int64_t out = 0;
	if (nzbget_rs_util_str_to_i64_checked(str.data(), str.size(), minVal, maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline std::optional<uint64_t> Util::StrToUnsignedChecked(std::string_view str, uint64_t maxVal)
{
	uint64_t out = 0;
	if (nzbget_rs_util_str_to_u64_checked(str.data(), str.size(), maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline int64_t Util::SafeCastSignedToSigned(int64_t num, int64_t minTo, int64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_i64_to_i64(num, minTo, maxTo);
}

inline int64_t Util::SafeCastUnsignedToSigned(uint64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_u64_to_i64(num, maxTo);
}

inline uint64_t Util::SafeCastSignedToUnsigned(int64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_i64_to_u64(num, maxTo);
}

inline uint64_t Util::SafeCastUnsignedToUnsigned(uint64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_u64_to_u64(num, maxTo);
}

inline bool Util::IsControlChar(char c) noexcept
{
	return nzbget_rs_util_is_control_char(static_cast<uint8_t>(c)) != 0;
}

inline CString Util::FormatSize(int64 fileSize)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_size(fileSize);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline CString Util::FormatSpeed(int64 bytesPerSecond)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_speed(bytesPerSecond);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline void Util::FormatTime(time_t timeSec, char* buffer, int bufsize)
{
	nzbget_rs_util_format_time(static_cast<int64_t>(timeSec), buffer, bufsize);
}

inline CString Util::FormatTime(time_t timeSec)
{
	CString result;
	result.Reserve(50);
	FormatTime(timeSec, result, 50);
	return result;
}

inline CString Util::FormatBuffer(const char* buf, int len)
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

inline bool Util::MatchFileExt(const char* filename, const char* extensionList, const char* listSeparator)
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

inline std::vector<CString> Util::SplitCommandLine(const char* commandLine)
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

inline void Util::TrimRight(char* str)
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

inline void Util::TrimRight(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 0, 1, 0, &start, TextRightSpace));
}

inline void Util::TrimLeft(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	nzbget_rs_trim_string(str.data(), str.size(), 1, 0, 0, &start, TextRightSpace);
	str.erase(0, start);
}

inline char* Util::Trim(char* str)
{
	// rust/src/util.rs
	return nzbget_rs_trim_line(str, 0);
}

inline void Util::Trim(std::string& str)
{
	TrimLeft(str);
	TrimRight(str);
}

inline void Util::SanitizeLine(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 1, 1, 1, &start, TextRightSpace));
	str.erase(0, start);
}

inline char* Util::ReduceStr(char* str, const char* from, const char* to)
{
	// rust/src/util.rs
	nzbget_rs_reduce_str(str, from, to);
	return str;
}

inline std::vector<CString> Util::SplitStr(const char* str, const char* separators)
{
	std::vector<CString> result;
	Tokenizer tok(str, separators);
	while (const char* substr = tok.Next())
	{
		result.emplace_back(substr);
	}
	return result;
}

inline bool Util::EndsWith(std::string_view str, std::string_view suffix, bool caseSensitive)
{
	// rust/src/util.rs
	return nzbget_rs_ends_with(str.data(), str.size(), suffix.data(), suffix.size(), caseSensitive) != 0;
}

inline bool Util::EndsWith(const char* str, const char* suffix, bool caseSensitive)
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

inline bool Util::AlphaNum(const char* str)
{
	// rust/src/util.rs
	return nzbget_rs_alpha_num(str) != 0;
}

inline uint32 Util::HashBJ96(const char* buffer, int bufSize, uint32 initValue)
{
	// rust/src/util.rs
	return nzbget_rs_hash_bj96(buffer, bufSize, initValue);
}

inline time_t Util::CurrentTime()
{
	return ::time(nullptr);
}

inline time_t Util::Timegm(tm const *t)
{
	return (time_t)nzbget_rs_ufs_util_internal_timegm(
		t->tm_year, t->tm_mon, t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec);
}

inline bool Util::StrCaseCmp(std::string_view a, std::string_view b)
{
	return nzbget_rs_dqh_slice_eq_ignore_case(
		a.data(), a.size(), b.data(), b.size()) != 0;
}

// prevent PC from going to sleep
inline void Util::SetStandByMode([[maybe_unused]] bool standBy)
{
}

inline int Util::NumberOfCpuCores()
{
	return std::thread::hardware_concurrency();
}

inline int64 Util::CurrentTicks()
{
	timeval t;
	gettimeofday(&t, nullptr);
	return (int64)(t.tv_sec) * 1000000ll + (int64)(t.tv_usec);
}

inline void Util::Sleep(int ms)
{
	std::this_thread::sleep_for(std::chrono::milliseconds(ms));
}

inline uint32 WebUtil::DecodeBase64(char* inputBuffer, int inputBufferLength, char* outputBuffer)
{
	// rust/src/decode.rs
	return nzbget_rs_decode_base64(inputBuffer, inputBufferLength, outputBuffer);
}

/* END - Base64
*/

inline CString WebUtil::XmlEncode(const char* raw)
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

inline void WebUtil::XmlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_decode(raw, XmlDigitLower);
}

inline const char* WebUtil::XmlFindTag(const char* xml, const char* tag, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_xml_find_tag(xml, tag, valueLength);
}

inline bool WebUtil::XmlParseTagValue(const char* xml, const char* tag, char* valueBuf, int valueBufSize, const char** tagEnd)
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

inline void WebUtil::XmlStripTags(char* xml)
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

inline void WebUtil::XmlRemoveEntities(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_remove_entities(raw, XmlEntityAlpha);
}

inline CString WebUtil::JsonEncode(const char* raw)
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

inline void WebUtil::JsonDecode(char* raw)
{
	// rust/src/decode.rs
	nzbget_rs_json_decode(raw);
}

inline const char* WebUtil::JsonFindField(const char* jsonText, const char* fieldName, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_json_find_field(jsonText, fieldName, valueLength);
}

inline const char* WebUtil::JsonNextValue(const char* jsonText, int* valueLength)
{
	// rust/src/decode.rs
	return nzbget_rs_json_next_value(jsonText, valueLength);
}

inline void WebUtil::HttpUnquote(char* raw)
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

inline CString WebUtil::ParseContentDispositionFilename(const char* contentDisposition)
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

inline void WebUtil::UrlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_url_decode(raw);
}

inline CString WebUtil::UrlEncode(const char* raw)
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

inline CString WebUtil::Latin1ToUtf8(const char* str)
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
inline time_t WebUtil::ParseRfc822DateTime(const char* dateTimeStr)
{
	// rust/src/util.rs
	return static_cast<time_t>(nzbget_rs_parse_rfc822_date_time(dateTimeStr));
}


inline URL::URL(const char* address) :
	m_address(address)
{
	if (address)
	{
		ParseUrl();
	}
}

inline void URL::ParseUrl()
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


inline RegEx::RegEx(const char *pattern, int matchBufSize)
{
#ifdef HAVE_REGEX_H
	bool valid = regcomp(&m_context, pattern, REG_EXTENDED | REG_ICASE | (matchBufSize > 0 ? 0 : REG_NOSUB)) == 0;
#else
	bool valid = false;
#endif
	m_rsRegex = nzbget_rs_regex_new(valid ? 1 : 0, matchBufSize);
}

inline RegEx::~RegEx()
{
	nzbget_rs_regex_free(m_rsRegex);
	m_rsRegex = nullptr;
#ifdef HAVE_REGEX_H
	regfree(&m_context);
#endif
}

inline bool RegEx::IsValid()
{
	return nzbget_rs_regex_is_valid(m_rsRegex) != 0;
}

inline bool RegEx::Match(const char *str)
{
#ifdef HAVE_REGEX_H
	struct ExecCtx
	{
		regex_t* context;
		const char* str;
	} ctx{&m_context, str};

	return nzbget_rs_regex_match(
		m_rsRegex, &ctx,
		[](void* raw, int32_t* outSo, int32_t* outEo, size_t nmatch) -> int {
			auto* c = static_cast<ExecCtx*>(raw);
			std::vector<regmatch_t> matches(nmatch);
			regmatch_t* pmatch = nmatch > 0 ? matches.data() : nullptr;
			if (regexec(c->context, c->str, nmatch, pmatch, 0) != 0)
			{
				return 0;
			}
			for (size_t i = 0; i < nmatch; i++)
			{
				outSo[i] = static_cast<int32_t>(matches[i].rm_so);
				outEo[i] = static_cast<int32_t>(matches[i].rm_eo);
			}
			return 1;
		}) != 0;
#else
	(void)str;
	return false;
#endif
}

inline int RegEx::GetMatchCount()
{
	return nzbget_rs_regex_get_match_count(m_rsRegex);
}

inline int RegEx::GetMatchStart(int index)
{
	return nzbget_rs_regex_get_match_start(m_rsRegex, index);
}

inline int RegEx::GetMatchLen(int index)
{
	return nzbget_rs_regex_get_match_len(m_rsRegex, index);
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

inline bool WildMask::Match(const char* text)
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
inline uint32 ZLib::GZipLen(int inputBufferLength)
{
	return nzbget_rs_gzip_len(inputBufferLength);
}

inline uint32 ZLib::GZip(const void* inputBuffer, int inputBufferLength, void* outputBuffer, int outputBufferLength)
{
	return nzbget_rs_gzip_compress(
		static_cast<const uint8_t*>(inputBuffer),
		inputBufferLength,
		static_cast<uint8_t*>(outputBuffer),
		outputBufferLength);
}

inline GUnzipStream::GUnzipStream(int BufferSize)
	: m_rsStream(nzbget_rs_gunzip_new(BufferSize))
{
}

inline GUnzipStream::~GUnzipStream()
{
	nzbget_rs_gunzip_free(static_cast<RsGUnzipStream*>(m_rsStream));
	m_rsStream = nullptr;
}

inline void GUnzipStream::Write(const void *inputBuffer, int inputBufferLength)
{
	nzbget_rs_gunzip_write(
		static_cast<RsGUnzipStream*>(m_rsStream),
		static_cast<const uint8_t*>(inputBuffer),
		inputBufferLength);
}

inline GUnzipStream::EStatus GUnzipStream::Read(const void **outputBuffer, int *outputBufferLength)
{
	int status = nzbget_rs_gunzip_read(
		static_cast<RsGUnzipStream*>(m_rsStream),
		outputBuffer,
		outputBufferLength);
	if (status == 1)
	{
		return zlFinished;
	}
	if (status == 2)
	{
		return zlOK;
	}
	return zlError;
}
#endif

inline Tokenizer::Tokenizer(const char* dataString, const char* separators) :
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

inline Tokenizer::Tokenizer(char* dataString, const char* separators, bool inplaceBuf) :
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

inline char* Tokenizer::Next()
{
	char* token = nzbget_rs_dqh_tokenizer_next(
		m_dataString, m_separators, m_working ? 1 : 0, &m_savePtr);
	m_working = true;
	return token;
}

inline void Crc32::Reset()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = 0;
}

inline void Crc32::Append(uchar* block, uint32 length)
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = nzbget_rs_crc32_append(block, length, *currentCrc);
}

inline uint32 Crc32::Finish()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	return *currentCrc;
}

inline uint32 Crc32::Combine(uint32 crc1, uint32 crc2, uint32 len2)
{
	// rust/src/crc.rs
	return nzbget_rs_crc32_combine(crc1, crc2, len2);
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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


#ifndef FILESYSTEM_H
#define FILESYSTEM_H

#include <optional>
#include <chrono>
#include <filesystem>

namespace fs
{
using namespace std::filesystem;
using error_code = std::error_code;
using errc = std::errc;

inline fs::path u8path(std::string_view pathStr)
{
	return fs::path(pathStr);
}

inline std::string u8string(const path& p)
{
	return p.string();
}
}



namespace fs
{
inline path make_unique_filename()
{
	return std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
}

fs::path make_unique_filename(const fs::path& targetPath);

void move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept;

inline void move_file(const fs::path& src, const fs::path& dest)
{
	error_code ec;
	move_file(src, dest, ec);
	if (ec) throw std::runtime_error(ec.message());
}
}

class FileSystem
{
	struct DiskState
	{
		int64 available;
		int64 total;
	};

public:
	static CString GetLastErrorMessage();
	static char* BaseFileName(const char* filename);
	static std::pair<std::string, std::string> SplitPathAndFilename(const std::string& fullPath);
	static bool SameFilename(const char* filename1, const char* filename2);
	static void NormalizePathSeparators(char* path);
	static std::optional<std::string> GetRealPath(const std::string& path);
	static bool LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull);
	static bool SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen);
	static bool AllocateFile(const char* filename, int64 size, bool sparse, CString& errmsg);
	static CString MakeValidFilename(const char* filename, bool allowSlashes = false);
	static std::string SanitizePathSegment(std::string_view name);
	static std::string SanitizeRelativePath(std::string_view path);
	static bool ReservedChar(char ch);
	static CString MakeUniqueFilename(const char* destDir, const char* basename);
	static bool MoveFile(const char* srcFilename, const char* dstFilename);
	static bool CopyFile(const char* srcFilename, const char* dstFilename);
	static bool DeleteFile(const char* filename);
	static bool FileExists(const char* filename);
	static bool DirectoryExists(const char* dirFilename);
	// a symbolic link (or a reparse point on Windows), not followed
	static bool IsLink(const char* filename);
	static bool CreateDirectory(const char* dirFilename);
	static bool CreateDirectoryExclusive(const char* dirFilename);
	static std::string ExtractFilePathFromCmd(const std::string& path);
	static std::string EscapePathForShell(const std::string& path);
	static std::optional<std::string> GetFileExtension(std::string_view filename);

	/* Delete empty directory */
	static bool RemoveDirectory(const char* dirFilename);

	/* Delete directory which is empty or contains only hidden files or directories */
	static bool DeleteDirectory(const char* dirFilename);

	static bool DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg);
	static bool ForceDirectories(const char* path, CString& errmsg);
	static bool CreateHardLink(const char *from, const char *to, CString& errmsg);
	static CString GetCurrentDirectory();
	static int64 FileSize(const char* filename);
	static std::optional<DiskState> GetDiskState(const char* path);
	static bool DirEmpty(const char* dirFilename);
	static bool RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName);
	static CString ExpandHomePath(const char* filename);
	static void FixExecPermission(const char* filename);
	static bool RestoreFileOrDirPermissions(const char* filename);
	static bool RestoreFilePermissions(const char* filename);
	static bool RestoreDirPermissions(const char* filename);
	static bool RestorePermissions(const char* filename, mode_t mode);
	static mode_t uMask;
	static CString ExpandFileName(const char* filename);
	static CString GetExeFileName(const char* argv0);

	/* Flush disk buffers for file with given descriptor */
	static bool FlushFileBuffers(int fileDescriptor, CString& errmsg);

	/* Flush disk buffers for file metadata (after file renaming) */
	static bool FlushDirBuffers(const char* filename, CString& errmsg);

	static CString MakeExtendedPath(const char* path, bool force);
};

class DirBrowser
{
public:
	DirBrowser(const char* path, bool snapshot = true);
	~DirBrowser();
	const char* Next();

private:
	DIR* m_dir = nullptr;
	struct dirent* m_findData;

	bool m_snapshot;
	typedef std::deque<CString> FileList;
	FileList m_snapshotFiles;
	FileList::iterator m_snapshotIter;

	const char* InternNext();
};

class DiskFile
{
public:
	enum EOpenMode
	{
		omRead, // file must exist
		omReadWrite, // file must exist
		omWrite, // create new or overwrite existing
		omAppend // create new or append to existing
	};

	enum ESeekOrigin
	{
		soSet,
		soCur,
		soEnd
	};

	DiskFile() = default;
	DiskFile(const DiskFile&) = delete;
	virtual ~DiskFile();
	bool Open(const char* filename, EOpenMode mode);
	bool Close();
	bool Active() { return m_file != nullptr; }
	int64 Read(void* buffer, int64 size);
	int64 Write(const void* buffer, int64 size);
	int64 Position();
	bool Seek(int64 position, ESeekOrigin origin = soSet);
	bool Eof();
	bool Error();
	int64 Print(const char* format, ...) PRINTF_SYNTAX(2);
	virtual char* ReadLine(char* buffer, int64 size);
	bool SetWriteBuffer(int size);
	bool Flush();
	bool Sync(CString& errmsg);
	int GetFileDescriptor() const { return m_file ? fileno(m_file) : -1; }

private:
	FILE* m_file = nullptr;
};

#include "nzbget_rs.h"
#include <exception>
#include <memory>

// Keep the platform's stat ABI and errno behavior (Rust metadata can use
// statx, whose availability probe changes errno, and different time widths).
static inline int RustStat(const char* path)
{
	struct stat buffer;
	if (stat(path, &buffer) != 0)
	{
		return -errno;
	}
	return S_ISREG(buffer.st_mode) ? 1 : S_ISDIR(buffer.st_mode) ? 2 : 0;
}

static inline int RustStatvfs(const char* path, uint64_t* bavail, uint64_t* blocks, uint64_t* frsize)
{
	struct statvfs diskdata;
	if (statvfs(path, &diskdata) != 0)
	{
		return 0;
	}
	*bavail = static_cast<uint64_t>(diskdata.f_bavail);
	*blocks = static_cast<uint64_t>(diskdata.f_blocks);
	*frsize = static_cast<uint64_t>(diskdata.f_frsize);
	return 1;
}

inline CString FileSystem::GetLastErrorMessage()
{
	// rust/src/fsops.rs: the XSI strerror_r (glibc's GNU strerror_r left the
	// buffer empty for every known error, so the messages had no reason)
	BString<1024> msg;
	nzbget_rs_error_message(errno, msg);
	return *msg;
}

inline void FileSystem::NormalizePathSeparators(char* path)
{
	// rust/src/paths.rs
	nzbget_rs_normalize_path_separators(path);
}

inline std::optional<std::string> FileSystem::GetRealPath(const std::string& path)
{
	NzbgetRsBuf buf{};
	if (nzbget_rs_fs_get_real_path(path.data(), path.size(), &buf) == 0)
	{
		nzbget_rs_free(buf);
		return std::nullopt;
	}
	return CopyRustPathText(buf);
}

inline bool FileSystem::ForceDirectories(const char* path, CString& errmsg)
{
	// rust/src/fsops.rs
	NzbgetRsBuf msg{};
	bool ok = nzbget_rs_force_directories(path, &msg, RustStat) != 0;
	errmsg.Clear();
	if (!ok)
	{
		errmsg = msg.data;
	}
	nzbget_rs_free(msg);
	return ok;
}

inline bool FileSystem::CreateHardLink(const char *from, const char *to, CString& errmsg)
{
	NzbgetRsBuf msg{};
	bool ok = nzbget_rs_fs_create_hard_link(from, to, RustStat, &msg) != 0;
	errmsg.Clear();
	if (!ok)
	{
		errmsg.Set(msg.data ? msg.data : "", static_cast<int>(msg.len));
	}
	nzbget_rs_free(msg);
	return ok;
}

inline CString FileSystem::GetCurrentDirectory()
{
	NzbgetRsBuf buf = nzbget_rs_fs_get_current_directory();
	CString res(buf.data ? buf.data : "", static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return res;
}

inline bool FileSystem::DirEmpty(const char* dirFilename)
{
	DirBrowser dir(dirFilename);
	return dir.Next() == nullptr;
}

inline bool FileSystem::LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull)
{
	return nzbget_rs_fs_load_file_into_buffer(
		filename,
		addTrailingNull ? 1 : 0,
		RustStat,
		&buffer,
		[](void* ctx, int bufSize) -> uint8_t* {
			auto* b = static_cast<CharBuffer*>(ctx);
			b->Reserve(bufSize);
			return reinterpret_cast<uint8_t*>(static_cast<char*>(*b));
		}) != 0;
}

inline bool FileSystem::SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen)
{
	return nzbget_rs_fs_save_buffer_into_file(
		filename, buffer, bufLen > 0 ? static_cast<size_t>(bufLen) : 0) != 0;
}

inline bool FileSystem::AllocateFile(const char* filename, int64 size, [[maybe_unused]] bool sparse, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_allocate_file(filename, size, &errBuf) != 0;
	if (errBuf.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline char* FileSystem::BaseFileName(const char* filename)
{
	// rust/src/paths.rs
	return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
}

inline std::pair<std::string, std::string> FileSystem::SplitPathAndFilename(const std::string& fullPath)
{
	// rust/src/paths.rs
	size_t slash = nzbget_rs_path_position(1, fullPath.data(), fullPath.size());
	if (slash == SIZE_MAX)
	{
		return std::make_pair(fullPath, "");
	}
	return std::make_pair(fullPath.substr(0, slash), fullPath.substr(slash + 1));
}

inline bool FileSystem::ReservedChar(char ch)
{
	// rust/src/paths.rs
	return nzbget_rs_reserved_char(ch) != 0;
}

//replace bad chars in filename
inline CString FileSystem::MakeValidFilename(const char* filename, bool allowSlashes)
{
	// rust/src/paths.rs
	NzbgetRsBuf buf = nzbget_rs_path_text(0, filename, strlen(filename), allowSlashes);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline std::string FileSystem::SanitizePathSegment(std::string_view name)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(1, name.data(), name.size(), 0));
}

inline std::string FileSystem::SanitizeRelativePath(std::string_view path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(2, path.data(), path.size(), 0));
}

inline CString FileSystem::MakeUniqueFilename(const char* destDir, const char* basename)
{
	// rust/src/fsops.rs
	NzbgetRsBuf buf = nzbget_rs_make_unique_filename(destDir, basename, RustStat);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline fs::path fs::make_unique_filename(const fs::path& targetPath)
{
	return fs::u8path(CopyRustPathText(nzbget_rs_fs_make_unique_path(targetPath.c_str())));
}

inline void fs::move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept
{
	int err = nzbget_rs_fs_move_file_no_overwrite(src.c_str(), dest.c_str());
	if (err == 0)
	{
		ec.clear();
	}
	else
	{
		ec = std::make_error_code(static_cast<std::errc>(err));
	}
}

inline bool FileSystem::MoveFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_move_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::CopyFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_copy_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::DeleteFile(const char* filename)
{
	return nzbget_rs_fs_delete_file(filename) != 0;
}

inline bool FileSystem::FileExists(const char* filename)
{
	// rust/src/fsops.rs (stat, a regular file)
	return nzbget_rs_file_exists(filename, RustStat) != 0;
}

inline bool FileSystem::DirectoryExists(const char* dirFilename)
{
	// rust/src/fsops.rs (stat, a directory)
	return nzbget_rs_directory_exists(dirFilename, RustStat) != 0;
}

inline bool FileSystem::CreateDirectory(const char* dirFilename)
{
	nzbget_rs_fs_create_directory_exclusive(dirFilename);
	return DirectoryExists(dirFilename);
}

inline bool FileSystem::CreateDirectoryExclusive(const char* dirFilename)
{
	return nzbget_rs_fs_create_directory_exclusive(dirFilename) != 0;
}

inline bool FileSystem::RemoveDirectory(const char* dirFilename)
{
	return nzbget_rs_fs_remove_directory(dirFilename) != 0;
}


inline std::string FileSystem::ExtractFilePathFromCmd(const std::string& path)
{
	// rust/src/paths.rs
	return path.substr(0, nzbget_rs_path_position(2, path.data(), path.size()));
}

inline std::string FileSystem::EscapePathForShell(const std::string& path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(3, path.data(), path.size(), 0));
}

inline std::optional<std::string> FileSystem::GetFileExtension(std::string_view filename)
{
	// rust/src/fsdelete.rs
	ptrdiff_t extIdx = nzbget_rs_file_extension(filename.data(), filename.size());
	if (extIdx < 0)
		return std::nullopt;

	return std::string(filename.substr(extIdx));
}

namespace
{
enum class PathType
{
	Error,
	File,
	Directory,
	Link,
	DirectoryLink
};

PathType GetPathTypeNoFollow(const char* filename)
{
	struct stat buffer;
	if (lstat(filename, &buffer) != 0)
	{
		return PathType::Error;
	}
	if (S_ISLNK(buffer.st_mode))
	{
		return PathType::Link;
	}
	return S_ISDIR(buffer.st_mode) ? PathType::Directory : PathType::File;
}

bool DeleteLink(const char* filename, PathType pathType)
{
	(void)pathType;
	return FileSystem::DeleteFile(filename);
}

// The calls rust/src/fsdelete.rs makes. errno is kept between the calls as
// the C++ kept it (Rust's own allocations don't touch it) and restored when
// the operation returns; an exception ends the operation and is rethrown.
class RsFsOps
{
public:
	NzbgetRsFsOps ops{};

	RsFsOps() : m_errno(errno)
	{
		ops.ctx = this;
		ops.pathType = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return static_cast<int>(GetPathTypeNoFollow(path)); });
		};
		ops.removeDir = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::RemoveDirectory(path) ? 1 : 0; });
		};
		ops.deleteFile = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DeleteFile(path) ? 1 : 0; });
		};
		ops.deleteLink = [](void* ctx, const char* path, int type) noexcept -> int
		{
			return Call(ctx, [&] { return DeleteLink(path, type == 4 ? PathType::DirectoryLink : PathType::Link) ? 1 : 0; });
		};
		ops.list = [](void* ctx, const char* dir, void* sink, void (*add)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&]
			{
				DirBrowser browser(dir);
				while (const char* filename = browser.Next())
				{
					add(sink, filename);
				}
				return 0;
			});
		};
		ops.fileExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::FileExists(path) ? 1 : 0; });
		};
		ops.dirExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DirectoryExists(path) ? 1 : 0; });
		};
		ops.moveFile = [](void* ctx, const char* from, const char* to) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::MoveFile(from, to) ? 1 : 0; });
		};
		ops.lastError = [](void* ctx, void* sink, void (*set)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&] { set(sink, FileSystem::GetLastErrorMessage()); return 0; });
		};
		ops.tooLong = [](void* ctx) noexcept -> int
		{
			return Call(ctx, [&] { errno = ENAMETOOLONG; return 0; });
		};
	}

	int Finish(int result)
	{
		errno = m_errno;
		if (m_error)
		{
			std::rethrow_exception(m_error);
		}
		return result;
	}

private:
	int m_errno;
	std::exception_ptr m_error;

	template <class F>
	static int Call(void* ctx, F f) noexcept
	{
		RsFsOps& self = *static_cast<RsFsOps*>(ctx);
		errno = self.m_errno;
		try
		{
			int result = f();
			self.m_errno = errno;
			return result;
		}
		catch (...)
		{
			self.m_errno = errno;
			self.m_error = std::current_exception();
			return -1;
		}
	}
};
}

inline bool FileSystem::IsLink(const char* filename)
{
	PathType type = GetPathTypeNoFollow(filename);
	return type == PathType::Link || type == PathType::DirectoryLink;
}

/* Delete directory which is empty or contains only hidden files or directories (whose names start with dot) */
inline bool FileSystem::DeleteDirectory(const char* dirFilename)
{
	if (!dirFilename)
	{
		errno = EFAULT;
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	return ops.Finish(nzbget_rs_delete_directory(&ops.ops, dirFilename, PATH_SEPARATOR)) == 1;
}

inline bool FileSystem::DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg)
{
	// Keep the POSIX NULL-path error distinct from an empty path, without
	// passing NULL to lstat or printf (their declarations require non-NULL).
	if (!dirFilename)
	{
		errmsg.Clear();
		errno = EFAULT;
		NzbgetRsBuf errBuf = nzbget_rs_dqh_fs_format_null_delete_errmsg(*GetLastErrorMessage());
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
		nzbget_rs_free(errBuf);
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf text{};
	int result = nzbget_rs_delete_directory_with_content(&ops.ops, dirFilename, PATH_SEPARATOR, &text);
	if (text.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(text.data, (int)text.len);
	}
	nzbget_rs_free(text);
	return ops.Finish(result) == 1;
}

inline int64 FileSystem::FileSize(const char* filename)
{
	return nzbget_rs_fs_file_size(filename);
}

inline std::optional<FileSystem::DiskState> FileSystem::GetDiskState(const char* path)
{
	int64 available = 0;
	int64 total = 0;
	if (nzbget_rs_fs_get_disk_state(path, RustStatvfs, &available, &total) != 0)
	{
		return FileSystem::DiskState{ available, total };
	}
	return std::nullopt;
}

inline bool FileSystem::RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName)
{
	if (!filename || !bakPart)
	{
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf name{};
	int result = nzbget_rs_rename_bak(&ops.ops, filename, bakPart, removeOldExtension, &name);
	newName.Set(name.data, (int)name.len);
	nzbget_rs_free(name);
	return ops.Finish(result) == 1;
}

inline CString FileSystem::ExpandHomePath(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_fs_expand_home_path(filename);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline CString FileSystem::ExpandFileName(const char* filename)
{
	NzbgetRsBuf expBuf = nzbget_rs_fs_expand_file_name(filename);
	CString result(expBuf.data ? expBuf.data : "", static_cast<int>(expBuf.len));
	nzbget_rs_free(expBuf);
	return result;
}

inline CString FileSystem::GetExeFileName(const char* argv0)
{
	NzbgetRsBuf exeBuf = nzbget_rs_fs_get_exe_file_name(argv0);
	CString result(exeBuf.data ? exeBuf.data : "", static_cast<int>(exeBuf.len));
	nzbget_rs_free(exeBuf);
	return result;
}

inline bool FileSystem::SameFilename(const char* filename1, const char* filename2)
{
	return nzbget_rs_dqh_bytes_equal(filename1, filename2) != 0;
}

inline bool FileSystem::FlushFileBuffers(int fileDescriptor, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_file_buffers(fileDescriptor, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline bool FileSystem::FlushDirBuffers(const char* filename, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_dir_buffers(filename, PATH_SEPARATOR, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline mode_t FileSystem::uMask;

inline void FileSystem::FixExecPermission(const char* filename)
{
	nzbget_rs_fs_fix_exec_permission(filename);
}

inline bool FileSystem::RestoreFileOrDirPermissions(const char* filename)
{
	return nzbget_rs_fs_restore_file_or_dir_permissions(filename, static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline bool FileSystem::RestoreFilePermissions(const char* filepath)
{
	return RestorePermissions(filepath, 0666);
}

inline bool FileSystem::RestoreDirPermissions(const char* filename)
{
	return RestorePermissions(filename, 0777);
}

inline bool FileSystem::RestorePermissions(const char* filename, mode_t mode)
{
	return nzbget_rs_fs_restore_permissions(
		filename, static_cast<uint32_t>(mode), static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline CString FileSystem::MakeExtendedPath(const char* path, [[maybe_unused]] bool force)
{
	return path;
}


inline DirBrowser::DirBrowser(const char* path, bool snapshot) :
	m_snapshot(snapshot)
{
	if (m_snapshot)
	{
		DirBrowser dir(path, false);
		while (const char* filename = dir.Next())
		{
			m_snapshotFiles.emplace_back(filename);
		}
		m_snapshotIter = m_snapshotFiles.begin();
	}
	else
	{
		m_dir = opendir(path);
	}
}

inline DirBrowser::~DirBrowser()
{
	if (m_dir)
	{
		closedir(m_dir);
	}
}

inline const char* DirBrowser::InternNext()
{
	if (m_snapshot)
	{
		return m_snapshotIter == m_snapshotFiles.end() ? nullptr : **m_snapshotIter++;
	}
	else
	{
		if (m_dir)
		{
			m_findData = readdir(m_dir);
			if (m_findData)
			{
				return m_findData->d_name;
			}
		}
		return nullptr;
	}
}

inline const char* DirBrowser::Next()
{
	const char* filename = nullptr;
	for (filename = InternNext(); filename && nzbget_rs_ufs_dir_browser_is_dot_or_dotdot(filename) != 0; )
	{
		filename = InternNext();
	}
	return filename;
}


inline DiskFile::~DiskFile()
{
	if (m_file)
	{
		Close();
	}
}

inline bool DiskFile::Open(const char* filename, EOpenMode mode)
{
	const char* strmode = mode == omRead ? FOPEN_RB : mode == omReadWrite ?
		FOPEN_RBP : mode == omWrite ? FOPEN_WB : FOPEN_AB;
	int fd = nzbget_rs_fs_open_cloexec_fd(
		filename, (int)mode, (int)omRead, (int)omReadWrite, (int)omWrite);
	if (fd == -1)
	{
		return false;
	}
	m_file = fdopen(fd, strmode);
	if (!m_file)
	{
		int error = errno;
		close(fd);
		errno = error;
	}
	return m_file;
}

inline bool DiskFile::Close()
{
	if (m_file)
	{
		int ret = fclose(m_file);
		m_file = nullptr;
		return ret == 0;
	}
	else
	{
		return false;
	}
}

inline int64 DiskFile::Read(void* buffer, int64 size)
{
	return fread(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Write(const void* buffer, int64 size)
{
	return fwrite(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Print(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vfprintf(m_file, format, ap);
	va_end(ap);
	return ret;
}

inline char* DiskFile::ReadLine(char* buffer, int64 size)
{
	return fgets(buffer, (int)size, m_file);
}

inline int64 DiskFile::Position()
{
	return ftell(m_file);
}

inline bool DiskFile::Seek(int64 position, ESeekOrigin origin)
{
	return fseek(m_file, position,
		nzbget_rs_ufs_disk_file_select_seek_origin(
			(int)origin, (int)soCur, (int)soEnd, SEEK_SET, SEEK_CUR, SEEK_END)) == 0;
}

inline bool DiskFile::Eof()
{
	return feof(m_file) != 0;
}

inline bool DiskFile::Error()
{
	return ferror(m_file) != 0;
}

inline bool DiskFile::SetWriteBuffer(int size)
{
	return setvbuf(m_file, nullptr, _IOFBF, size) == 0;
}

inline bool DiskFile::Flush()
{
	return fflush(m_file) == 0;
}

inline bool DiskFile::Sync(CString& errmsg)
{
	return FileSystem::FlushFileBuffers(fileno(m_file), errmsg);
}

#endif

#endif
#endif
