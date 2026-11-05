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

#include "FeedsValidator.h"
#include "FeedValidator.h"
#include "nzbget_rs.h"

namespace SystemHealth::Feeds
{
FeedsValidator::FeedsValidator(const ::Feeds& feeds, const Options& options)
	: SectionGroupValidator(MakeFeedValidators(feeds, options)), m_feeds(feeds), m_options(options)
{
	m_validators.reserve(1);
	m_validators.push_back(std::make_unique<DuplicateFeedsValidator>(feeds));
}
std::vector<std::unique_ptr<SectionValidator>> FeedsValidator::MakeFeedValidators(
	const ::Feeds& feeds, const Options& options) const
{
	std::vector<std::unique_ptr<SectionValidator>> validators;
	validators.reserve(feeds.size());
	for (const auto& feed : feeds)
	{
		validators.push_back(std::make_unique<FeedValidator>(*feed, options));
	}
	return validators;
}

Status DuplicateFeedsValidator::Validate() const
{
	if (m_feeds.empty()) return Status::Ok();

	std::vector<const char*> namePtrs(m_feeds.size());
	std::vector<size_t> nameLens(m_feeds.size());
	std::vector<const char*> urlPtrs(m_feeds.size());
	std::vector<size_t> urlLens(m_feeds.size());
	for (size_t i = 0; i < m_feeds.size(); ++i)
	{
		std::string_view name = m_feeds[i]->GetName();
		std::string_view url = m_feeds[i]->GetUrl();
		namePtrs[i] = name.data();
		nameLens[i] = name.size();
		urlPtrs[i] = url.data();
		urlLens[i] = url.size();
	}
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_duplicate_feed_urls(
		namePtrs.data(), nameLens.data(),
		urlPtrs.data(), urlLens.data(),
		m_feeds.size(), &msg);
	return Status::FromRust(sev, msg);
}
}  // namespace SystemHealth::Feeds
