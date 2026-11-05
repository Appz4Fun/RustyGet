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

#include "DownloadQueueValidator.h"
#include "Options.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::DownloadQueue
{
DownloadQueueValidator::DownloadQueueValidator(const Options& options) : m_options(options)
{
}

SectionReport DownloadQueueValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	nzbget_rs_health_validate_download_queue_section(
		m_options.GetFlushQueue() ? 1 : 0,
		m_options.GetContinuePartial() ? 1 : 0,
		m_options.GetPropagationDelay(),
		m_options.GetArticleCache(),
		m_options.GetDirectWrite() ? 1 : 0,
		m_options.GetWriteBuffer(),
		static_cast<int>(m_options.GetFileNaming()),
		static_cast<int>(m_options.GetPostStrategy()),
		m_options.GetDiskSpace(),
		m_options.GetNzbCleanupDisk() ? 1 : 0,
		m_options.GetKeepHistory(),
		m_options.GetFeedHistory(),
		m_options.GetSkipWrite() ? 1 : 0,
		m_options.GetRawArticle() ? 1 : 0,
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}
}  // namespace SystemHealth::DownloadQueue
