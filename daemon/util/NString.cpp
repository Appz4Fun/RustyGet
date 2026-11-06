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

#include "nzbget.h"
#include "NString.h"
#include "nzbget_rs.h"
#include <vector>

namespace
{
struct VsnprintfCtx
{
	const char* format;
	va_list* ap;
};

int RunVsnprintf(void* ctx, char* dst, size_t size)
{
	auto* v = static_cast<VsnprintfCtx*>(ctx);
	return vsnprintf(dst, size, v->format, *v->ap);
}
}

template <int size>
BString<size>::BString(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	FormatV(format, ap);
	va_end(ap);
}

template <int size>
void BString<size>::Set(const char* str, int len)
{
	nzbget_rs_bstring_set(m_data, size, str, len);
}

template <int size>
void BString<size>::Append(const char* str, int len)
{
	nzbget_rs_bstring_append(m_data, size, str, len);
}

template <int size>
void BString<size>::AppendFmt(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	AppendFmtV(format, ap);
	va_end(ap);
}

template <int size>
void BString<size>::AppendFmtV(const char* format, va_list ap)
{
	va_list apCopy;
	va_copy(apCopy, ap);
	VsnprintfCtx ctx{ format, &apCopy };
	nzbget_rs_bstring_append_fmt(m_data, size, &ctx, RunVsnprintf);
	va_end(apCopy);
}

template <int size>
int BString<size>::Format(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int len = FormatV(format, ap);
	va_end(ap);
	return len;
}

template <int size>
int BString<size>::FormatV(const char* format, va_list ap)
{
	va_list apCopy;
	va_copy(apCopy, ap);
	VsnprintfCtx ctx{ format, &apCopy };
	int len = nzbget_rs_bstring_format(m_data, size, &ctx, RunVsnprintf);
	va_end(apCopy);
	return len;
}

bool CString::operator==(const CString& other) const
{
	return nzbget_rs_cstring_eq(m_data, other.m_data) != 0;
}

bool CString::operator==(const char* other) const
{
	return nzbget_rs_cstring_eq(m_data, other) != 0;
}

void CString::Clear()
{
	nzbget_rs_cstring_clear(&m_data);
}

void CString::Set(const char* str, int len)
{
	nzbget_rs_cstring_set(&m_data, str, len);
}

void CString::Append(const char* str, int len)
{
	nzbget_rs_cstring_append(&m_data, str, len);
}

void CString::AppendFmt(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	AppendFmtV(format, ap);
	va_end(ap);
}

void CString::AppendFmtV(const char* format, va_list ap)
{
	va_list ap2;
	va_copy(ap2, ap);
	int addLen = vsnprintf(nullptr, 0, format, ap);
	if (addLen > 0)
	{
		VsnprintfCtx ctx{ format, &ap2 };
		nzbget_rs_cstring_append_fmt(&m_data, addLen, &ctx, RunVsnprintf);
	}
	va_end(ap2);
}

int CString::Format(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int len = FormatV(format, ap);
	va_end(ap);
	return len;
}

int CString::FormatV(const char* format, va_list ap)
{
	va_list ap2;
	va_copy(ap2, ap);
	int newLen = vsnprintf(nullptr, 0, format, ap);
	int res = 0;
	if (newLen >= 0)
	{
		VsnprintfCtx ctx{ format, &ap2 };
		res = nzbget_rs_cstring_format(&m_data, newLen, &ctx, RunVsnprintf);
	}
	va_end(ap2);
	return res;
}

int CString::Find(const char* str, int pos)
{
	return nzbget_rs_cstring_find(m_data, str, pos);
}

void CString::Replace(int pos, int len, const char* str, int strLen)
{
	nzbget_rs_cstring_replace_range(&m_data, pos, len, str, strLen);
}

void CString::Replace(const char* from, const char* to)
{
	nzbget_rs_cstring_replace_all(&m_data, from, to);
}

void CString::Bind(char* str)
{
	nzbget_rs_cstring_bind(&m_data, str);
}

char* CString::Unbind()
{
	return nzbget_rs_cstring_unbind(&m_data);
}

void CString::Reserve(int capacity)
{
	nzbget_rs_cstring_reserve(&m_data, capacity);
}

CString CString::FormatStr(const char* format, ...)
{
	CString result;
	va_list ap;
	va_start(ap, format);
	result.FormatV(format, ap);
	va_end(ap);
	return result;
}

void CString::TrimRight()
{
	nzbget_rs_cstring_trim_right(m_data);
}

WString::WString(const char* utfstr)
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

void StringBuilder::Clear()
{
	nzbget_rs_string_builder_clear(&m_data, &m_length, &m_capacity);
}

char* StringBuilder::Unbind()
{
	return nzbget_rs_string_builder_unbind(&m_data, &m_length, &m_capacity);
}

void StringBuilder::Reserve(int capacity, bool exact)
{
	nzbget_rs_string_builder_reserve(&m_data, &m_capacity, capacity, exact ? 1 : 0);
}

void StringBuilder::Append(const char* str, int len)
{
	nzbget_rs_string_builder_append(&m_data, &m_length, &m_capacity, str, len);
}

void StringBuilder::AppendFmt(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	AppendFmtV(format, ap);
	va_end(ap);
}

void StringBuilder::AppendFmtV(const char* format, va_list ap)
{
	va_list ap2;
	va_copy(ap2, ap);
	int addLen = vsnprintf(nullptr, 0, format, ap);
	if (addLen > 0)
	{
		VsnprintfCtx ctx{ format, &ap2 };
		nzbget_rs_string_builder_append_fmt(
			&m_data, &m_length, &m_capacity, addLen, &ctx, RunVsnprintf);
	}
	va_end(ap2);
}

void CharBuffer::Reserve(int size)
{
	nzbget_rs_char_buffer_reserve(&m_data, &m_size, size);
}

void CharBuffer::Clear()
{
	nzbget_rs_char_buffer_clear(&m_data, &m_size);
}

// Instantiate all classes used in our project
template class BString<1024>;
template class BString<100>;
template class BString<20>;
