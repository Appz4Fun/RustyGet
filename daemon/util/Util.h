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
#include "FileSystem.h"

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

#endif
#endif
