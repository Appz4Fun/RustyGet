/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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

#include "ConnectionValidator.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::Connection
{
SectionReport ConnectionValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	nzbget_rs_health_validate_connection_section(
		m_options.GetArticleRetries(),
		m_options.GetHealthCheck() == Options::hcDupe ? 1 : 0,
		m_options.GetDupeSearch() ? 1 : 0,
		m_options.GetArticleInterval(),
		m_options.GetArticleTimeout(),
		m_options.GetArticleReadChunkSize(),
		m_options.GetUrlRetries(),
		m_options.GetUrlInterval(),
		m_options.GetUrlTimeout(),
		m_options.GetUrlConnections(),
		m_options.GetRemoteTimeout(),
		m_options.GetDownloadRate(),
		m_options.GetMonthlyQuota(),
		m_options.GetQuotaStartDay(),
		m_options.GetDailyQuota(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status ArticleRetriesValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_article_retries(
		m_options.GetArticleRetries(),
		m_options.GetHealthCheck() == Options::hcDupe ? 1 : 0,
		m_options.GetDupeSearch() ? 1 : 0,
		&msg);
	return Status::FromRust(sev, msg);
}

Status ArticleReadChunkSizeValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_article_read_chunk_size(
		m_options.GetArticleReadChunkSize(), &msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::Connection
