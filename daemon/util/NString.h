/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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


#ifndef NSTRING_H
#define NSTRING_H

#include <vector>
#include "nzbget_rs.h"

namespace nstring_detail
{
struct VsnprintfCtx
{
	const char* format;
	va_list* ap;
};

inline int RunVsnprintf(void* ctx, char* dst, size_t size)
{
	auto* v = static_cast<VsnprintfCtx*>(ctx);
	return vsnprintf(dst, size, v->format, *v->ap);
}
}

/*
BString is a replacement for char-arrays allocated on stack.
It has no memory overhead, provides memory management and formatting functions.
 */
template <int size>
class BString
{
public:
	BString() { m_data[0] = '\0'; }
	explicit BString(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		FormatV(format, ap);
		va_end(ap);
	}
	BString(BString& other) = delete;
	BString(const char* str) { Set(str); } // for initialization via assignment
	BString(BString&& other) noexcept { Set(other.m_data); } // never used but declaration is needed for initialization via assignment
	BString& operator=(const char* str) { Set(str); return *this; }
	int Length() const { return (int)strlen(m_data); }
	int Capacity() const { return size - 1; }
	bool Empty() const { return !*m_data; }
	void Clear() { m_data[0] = '\0'; }
	const char* Str() const { return m_data; }
	operator char*() const { return const_cast<char*>(m_data); }
	char* operator*() const { return const_cast<char*>(m_data); }
	void Set(const char* str, int len = 0)
	{
		nzbget_rs_bstring_set(m_data, size, str, len);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_bstring_append(m_data, size, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list apCopy;
		va_copy(apCopy, ap);
		nstring_detail::VsnprintfCtx ctx{ format, &apCopy };
		nzbget_rs_bstring_append_fmt(m_data, size, &ctx, nstring_detail::RunVsnprintf);
		va_end(apCopy);
	}
	int Format(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		int len = FormatV(format, ap);
		va_end(ap);
		return len;
	}
	int FormatV(const char* format, va_list ap)
	{
		va_list apCopy;
		va_copy(apCopy, ap);
		nstring_detail::VsnprintfCtx ctx{ format, &apCopy };
		int len = nzbget_rs_bstring_format(m_data, size, &ctx, nstring_detail::RunVsnprintf);
		va_end(apCopy);
		return len;
	}

protected:
	char m_data[size];
};

/*
CString is a replacement for C-Style null-terminated strings.
It has no memory overhead, provides memory management and string handling functions.
 */
class CString
{
public:
	CString() {}
	~CString()
	{
		Clear();
	}
	CString(const char* str, int len = 0) { Set(str, len); }
	CString(CString&& other) noexcept
	{
		m_data = other.m_data;
		other.m_data = nullptr;
	}
	CString(CString& other) = delete;
	CString& operator=(const CString& other) = delete;
	CString& operator=(CString&& other) noexcept
	{
		Clear();
		m_data = other.m_data;
		other.m_data = nullptr;
		return *this;
	}
	CString& operator=(const char* str) { Set(str); return *this; }
	bool operator==(const CString& other) const
	{
		return nzbget_rs_cstring_eq(m_data, other.m_data) != 0;
	}
	bool operator==(const char* other) const
	{
		return nzbget_rs_cstring_eq(m_data, other) != 0;
	}
	static CString FormatStr(const char* format, ...)
	{
		CString result;
		va_list ap;
		va_start(ap, format);
		result.FormatV(format, ap);
		va_end(ap);
		return result;
	}
	operator char*() const { return m_data; }
	char* operator*() const { return m_data; }
	const char* Str() const { return m_data ? m_data : ""; }
	int Length() const { return m_data ? (int)strlen(m_data) : 0; }
	bool Empty() const { return !m_data || !*m_data; }
	void Clear()
	{
		nzbget_rs_cstring_clear(&m_data);
	}
	void Reserve(int capacity)
	{
		nzbget_rs_cstring_reserve(&m_data, capacity);
	}
	void Bind(char* str)
	{
		nzbget_rs_cstring_bind(&m_data, str);
	}
	char* Unbind()
	{
		return nzbget_rs_cstring_unbind(&m_data);
	}
	void Set(const char* str, int len = 0)
	{
		nzbget_rs_cstring_set(&m_data, str, len);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_cstring_append(&m_data, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int addLen = vsnprintf(nullptr, 0, format, ap);
		if (addLen > 0)
		{
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			nzbget_rs_cstring_append_fmt(&m_data, addLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
	}
	int Format(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		int len = FormatV(format, ap);
		va_end(ap);
		return len;
	}
	int FormatV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int newLen = vsnprintf(nullptr, 0, format, ap);
		int res = 0;
		if (newLen >= 0)
		{
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			res = nzbget_rs_cstring_format(&m_data, newLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
		return res;
	}
	int Find(const char* str, int pos = 0)
	{
		return nzbget_rs_cstring_find(m_data, str, pos);
	}
	void Replace(int pos, int len, const char* str, int strLen = 0)
	{
		nzbget_rs_cstring_replace_range(&m_data, pos, len, str, strLen);
	}
	void Replace(const char* from, const char* to)
	{
		nzbget_rs_cstring_replace_all(&m_data, from, to);
	}
	void TrimRight()
	{
		nzbget_rs_cstring_trim_right(m_data);
	}

protected:
	char* m_data = nullptr;
};

/*
Wide-character string.
 */
class WString
{
public:
	WString(wchar_t* wstr) : m_data(wcsdup(wstr)) {}
	WString(const char* utfstr)
	{
		size_t maxUnits = strlen(utfstr) * 2;
		wchar_t* data = static_cast<wchar_t*>(malloc((maxUnits + 1) * sizeof(wchar_t)));
		if (!data) return;

		m_data = data;

		std::vector<uint16_t> tmp(maxUnits + 1, 0);
		size_t written = nzbget_rs_rco_wstring_decode_utf8_to_utf16(
			utfstr, tmp.data(), maxUnits);
		for (size_t i = 0; i < written; ++i)
		{
			m_data[i] = (wchar_t)tmp[i];
		}
		m_data[written] = '\0';
	}
	~WString() 
	{ 
		free(m_data);
		m_data = nullptr;
	}
	WString(WString&& other) noexcept { m_data = other.m_data; other.m_data = nullptr; }
	WString(const WString& other) = delete;
	WString& operator=(const WString& other) = delete;
	WString& operator=(WString&& other) = delete;
	operator wchar_t*() const { return m_data; }
	wchar_t* operator*() const { return m_data; }
	int Length() { return wcslen(m_data); }

protected:
	wchar_t* m_data = nullptr;
};

/*
StringBuilder preallocates storage space and is best suitable for often "Append"s.
 */
class StringBuilder
{
public:
	~StringBuilder()
	{
		Clear();
	}
	StringBuilder() = default;
	StringBuilder(const StringBuilder& other) = delete;
	StringBuilder& operator=(const StringBuilder& other) = delete;
	StringBuilder& operator=(StringBuilder&& other) = delete;
	operator const char*() const { return m_data ? m_data : ""; }
	explicit operator char*() { return m_data; }
	const char* operator*() const { return m_data; }
	int Length() const { return m_length; }
	void SetLength(int length) { m_length = length; }
	int Capacity() const { return m_capacity; }
	void Reserve(int capacity, bool exact = false)
	{
		nzbget_rs_string_builder_reserve(&m_data, &m_capacity, capacity, exact ? 1 : 0);
	}
	bool Empty() const { return m_length == 0; }
	void Clear()
	{
		nzbget_rs_string_builder_clear(&m_data, &m_length, &m_capacity);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_string_builder_append(&m_data, &m_length, &m_capacity, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int addLen = vsnprintf(nullptr, 0, format, ap);
		if (addLen > 0)
		{
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			nzbget_rs_string_builder_append_fmt(
				&m_data, &m_length, &m_capacity, addLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
	}
	char* Unbind()
	{
		return nzbget_rs_string_builder_unbind(&m_data, &m_length, &m_capacity);
	}

protected:
	char* m_data = nullptr;
	int m_length = 0;
	int m_capacity = 0;
};

/*
Plain char-buffer for I/O operations.
 */
class CharBuffer
{
public:
	CharBuffer() {}
	CharBuffer(int size) { Reserve(size); }
	CharBuffer(CharBuffer& other) : m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	~CharBuffer() { Clear(); }
	CharBuffer& operator=(CharBuffer&& other) = delete;
	int Size() { return m_size; }
	void Reserve(int size)
	{
		nzbget_rs_char_buffer_reserve(&m_data, &m_size, size);
	}
	void Clear()
	{
		nzbget_rs_char_buffer_clear(&m_data, &m_size);
	}
	operator char*() const { return m_data; }
	char* operator*() const { return m_data; }

protected:
	char* m_data = nullptr;
	int m_size = 0;
};

#ifdef DEBUG
// helper declarations to identify incorrect calls to "free" at compile time
void free(CString str);
void free(StringBuilder str);
void free(CharBuffer str);
#endif

#endif
