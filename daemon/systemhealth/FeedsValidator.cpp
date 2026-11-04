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

	std::vector<const char*> ptrs(m_feeds.size());
	std::vector<size_t> lens(m_feeds.size());
	for (size_t i = 0; i < m_feeds.size(); ++i)
	{
		std::string_view url = m_feeds[i]->GetUrl();
		ptrs[i] = url.data();
		lens[i] = url.size();
	}
	size_t firstIdx = 0;
	size_t dupIdx = 0;
	if (nzbget_rs_awc_health_find_duplicate_feed_url(
			ptrs.data(), lens.data(), m_feeds.size(), &firstIdx, &dupIdx) != 0)
	{
		return Status::Warning("Feeds '" + std::string(m_feeds[firstIdx]->GetName()) + "' and '" +
							   std::string(m_feeds[dupIdx]->GetName()) + "' have the same URL");
	}
	return Status::Ok();
}
}  // namespace SystemHealth::Feeds
