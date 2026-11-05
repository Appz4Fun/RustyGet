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

#include "FeedValidator.h"
#include "FeedInfo.h"
#include "Options.h"
#include "nzbget_rs.h"

namespace SystemHealth::Feeds
{

FeedValidator::FeedValidator(const FeedInfo& feed, const Options& options)
	: m_feed(feed), m_options(options), m_name("Feed" + std::to_string(feed.GetId()))
{
}

SectionReport FeedValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string_view name = m_feed.GetName();
	std::string_view url = m_feed.GetUrl();
	nzbget_rs_health_validate_feed_section(
		name.data(), name.size(),
		url.data(), url.size(),
		m_feed.GetInterval(),
		static_cast<uint32_t>(m_feed.GetCertVerificationLevel()),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status FeedCertValidator::Validate() const
{
	std::string_view url = m_feed.GetUrl();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_feed_cert(
		url.data(), url.size(),
		static_cast<uint32_t>(m_feed.GetCertVerificationLevel()),
		&msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::Feeds
