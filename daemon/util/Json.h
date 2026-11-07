/*
 *  This file is part of RustyGet.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

#ifndef JSON_H
#define JSON_H

#include <cstdint>
#include <iostream>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

namespace Json
{
	struct string
	{
		static constexpr size_t max_size() noexcept { return 0x7ffffffeu; }
	};

	class JsonValue;

	class JsonArray
	{
	public:
		static constexpr size_t max_size() noexcept { return 0x7fffffffu / 24u; }

		void push_back(JsonValue val);
		const std::vector<JsonValue>& items() const noexcept { return m_items; }

	private:
		std::vector<JsonValue> m_items;
	};

	class JsonObject
	{
	public:
		static constexpr size_t max_size() noexcept { return 0x3fffffffu; }

		JsonValue& operator[](std::string_view key);
		const std::vector<std::pair<std::string, JsonValue>>& entries() const noexcept { return m_entries; }

	private:
		std::vector<std::pair<std::string, JsonValue>> m_entries;
	};

	class JsonValue
	{
	public:
		using Storage = std::variant<std::monostate, bool, int64_t, uint64_t, double, std::string, JsonArray, JsonObject>;

		JsonValue() = default;
		JsonValue(bool v) : m_data(v) {}
		JsonValue(int v) : m_data(static_cast<int64_t>(v)) {}
		JsonValue(long v) : m_data(static_cast<int64_t>(v)) {}
		JsonValue(long long v) : m_data(static_cast<int64_t>(v)) {}
		JsonValue(unsigned int v) : m_data(static_cast<uint64_t>(v)) {}
		JsonValue(unsigned long v) : m_data(static_cast<uint64_t>(v)) {}
		JsonValue(unsigned long long v) : m_data(static_cast<uint64_t>(v)) {}
		JsonValue(double v) : m_data(v) {}
		JsonValue(const char* v) : m_data(std::string(v ? v : "")) {}
		JsonValue(std::string_view v) : m_data(std::string(v)) {}
		JsonValue(std::string v) : m_data(std::move(v)) {}
		JsonValue(JsonArray v) : m_data(std::move(v)) {}
		JsonValue(JsonObject v) : m_data(std::move(v)) {}

		const JsonObject& as_object() const
		{
			if (const auto* obj = std::get_if<JsonObject>(&m_data))
			{
				return *obj;
			}
			throw std::runtime_error("not an object");
		}

		const Storage& storage() const noexcept { return m_data; }

	private:
		Storage m_data;
	};

	inline void JsonArray::push_back(JsonValue val)
	{
		m_items.push_back(std::move(val));
	}

	inline JsonValue& JsonObject::operator[](std::string_view key)
	{
		for (auto& [k, v] : m_entries)
		{
			if (k == key)
			{
				return v;
			}
		}
		m_entries.emplace_back(std::string(key), JsonValue{});
		return m_entries.back().second;
	}

	std::string serialize(const JsonValue& val) noexcept;
	std::string serialize(const JsonObject& json) noexcept;
	std::string Serialize(const JsonObject& json) noexcept;
}

#endif
