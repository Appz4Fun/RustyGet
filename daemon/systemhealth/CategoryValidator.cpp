/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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

#include "CategoryValidator.h"
#include "Options.h"
#include "nzbget_rs.h"

namespace SystemHealth::Category
{

CategoryValidator::CategoryValidator(const Options& options, const Options::Category& category,
									 size_t id)
	: m_options(options), m_category(category), m_name("Category" + std::to_string(id))
{
}

SectionReport CategoryValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string_view name = m_category.GetName();
	nzbget_rs_health_validate_category_section(
		name.data(), name.size(),
		m_options.GetUnpack() ? 1 : 0,
		m_category.GetUnpack() ? 1 : 0,
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}
}  // namespace SystemHealth::Category
