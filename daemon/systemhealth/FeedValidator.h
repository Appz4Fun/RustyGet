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

#ifndef FEED_VALIDATOR_H
#define FEED_VALIDATOR_H

#include "SectionValidator.h"
#include "FeedInfo.h"
#include "Validators.h"
#include "Options.h"

namespace SystemHealth::Feeds
{

class FeedValidator final : public SectionValidator
{
public:
	FeedValidator(const FeedInfo& feed, const Options& options);
	std::string_view GetName() const override { return m_name; }
	SectionReport Validate() const override;

private:
	const FeedInfo& m_feed;
	const Options& m_options;
	const std::string m_name;
};

class FeedCertValidator final : public Validator
{
public:
	explicit FeedCertValidator(const FeedInfo& feed) : m_feed(feed) {}

	std::string_view GetName() const override { return "CertVerification"; }
	Status Validate() const override;

private:
	const FeedInfo& m_feed;
};

}  // namespace SystemHealth::Feeds

#endif
