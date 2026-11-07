/*
 *  This file is part of RustyGet.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

#include "nzbget.h"

#include "Json.h"
#include <cstdio>

namespace Json
{
	namespace
	{
		void AppendEscapedString(std::string& out, std::string_view str)
		{
			out.push_back('"');
			for (unsigned char c : str)
			{
				switch (c)
				{
				case '"':  out.append("\\\""); break;
				case '\\': out.append("\\\\"); break;
				case '\b': out.append("\\b"); break;
				case '\f': out.append("\\f"); break;
				case '\n': out.append("\\n"); break;
				case '\r': out.append("\\r"); break;
				case '\t': out.append("\\t"); break;
				default:
					if (c < 0x20)
					{
						char buf[7];
						snprintf(buf, sizeof(buf), "\\u%04x", static_cast<unsigned>(c));
						out.append(buf);
					}
					else
					{
						out.push_back(static_cast<char>(c));
					}
					break;
				}
			}
			out.push_back('"');
		}

		void AppendValue(std::string& out, const JsonValue& val);

		void AppendArray(std::string& out, const JsonArray& arr)
		{
			out.push_back('[');
			bool first = true;
			for (const auto& item : arr.items())
			{
				if (!first) out.push_back(',');
				first = false;
				AppendValue(out, item);
			}
			out.push_back(']');
		}

		void AppendObject(std::string& out, const JsonObject& obj)
		{
			out.push_back('{');
			bool first = true;
			for (const auto& [k, v] : obj.entries())
			{
				if (!first) out.push_back(',');
				first = false;
				AppendEscapedString(out, k);
				out.push_back(':');
				AppendValue(out, v);
			}
			out.push_back('}');
		}

		void AppendValue(std::string& out, const JsonValue& val)
		{
			std::visit([&out](const auto& v) {
				using T = std::decay_t<decltype(v)>;
				if constexpr (std::is_same_v<T, std::monostate>)
				{
					out.append("null");
				}
				else if constexpr (std::is_same_v<T, bool>)
				{
					out.append(v ? "true" : "false");
				}
				else if constexpr (std::is_same_v<T, int64_t> || std::is_same_v<T, uint64_t> || std::is_same_v<T, double>)
				{
					out.append(std::to_string(v));
				}
				else if constexpr (std::is_same_v<T, std::string>)
				{
					AppendEscapedString(out, v);
				}
				else if constexpr (std::is_same_v<T, JsonArray>)
				{
					AppendArray(out, v);
				}
				else if constexpr (std::is_same_v<T, JsonObject>)
				{
					AppendObject(out, v);
				}
			}, val.storage());
		}
	}

	std::string serialize(const JsonValue& val) noexcept
	{
		std::string out;
		AppendValue(out, val);
		return out;
	}

	std::string serialize(const JsonObject& json) noexcept
	{
		std::string out;
		AppendObject(out, json);
		return out;
	}

	std::string Serialize(const JsonObject& json) noexcept
	{
		return serialize(json);
	}
}
