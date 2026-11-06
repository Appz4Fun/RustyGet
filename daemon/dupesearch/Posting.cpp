/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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

#include "Posting.h"
#include "nzbget_rs.h"

// rust/src/posting.rs
namespace
{
struct Strings
{
	std::vector<const char*> data;
	std::vector<size_t> lens;
	explicit Strings(const std::vector<std::string>& texts)
	{
		for (const std::string& t : texts) { data.push_back(t.data()); lens.push_back(t.size()); }
	}
};

std::vector<NzbgetRsListing> Listings(const std::vector<Newznab::Result>& results)
{
	std::vector<NzbgetRsListing> raw;
	for (const Newznab::Result& r : results)
	{
		raw.push_back({r.link.data(), r.link.size(), (int64_t)r.size, (int32_t)r.grabs, (int64_t)r.date});
	}
	return raw;
}

std::vector<Posting::Group> Groups(const std::vector<Newznab::Result>& results, const std::vector<size_t>& indices,
	const std::vector<size_t>& sizes, size_t count)
{
	std::vector<Posting::Group> groups;
	size_t at = 0;
	for (size_t g = 0; g < count; g++)
	{
		Posting::Group group;
		for (size_t k = 0; k < sizes[g]; k++) group.push_back(results[indices[at++]]);
		groups.push_back(std::move(group));
	}
	return groups;
}
}

Posting::Sketch Posting::MakeSketch(const std::vector<std::string>& messageIds)
{
	Strings ids(messageIds);
	Sketch sketch(SketchSize);
	size_t len = 0;
	nzbget_rs_posting_sketch(ids.data.data(), ids.lens.data(), ids.data.size(), sketch.data(), &len);
	sketch.resize(len);
	return sketch;
}

bool Posting::SketchOfFile(const std::string& path, Sketch& sketch)
{
	sketch.assign(SketchSize, 0);
	size_t len = 0;
	if (!nzbget_rs_posting_sketch_of_file(path.c_str(), sketch.data(), SketchSize, &len))
	{
		sketch.clear();
		return false;
	}
	sketch.resize(len);
	return true;
}

bool Posting::SameSketch(const Sketch& a, const Sketch& b)
{
	return nzbget_rs_posting_same_sketch(a.data(), a.size(), b.data(), b.size()) != 0;
}

bool Posting::SamePosting(const std::vector<std::string>& a, const std::vector<std::string>& b)
{
	Strings x(a), y(b);
	return nzbget_rs_posting_same_posting(x.data.data(), x.lens.data(), x.data.size(), y.data.data(), y.lens.data(),
		y.data.size()) != 0;
}

std::vector<Posting::Group> Posting::GroupListings(std::vector<Newznab::Result> results)
{
	std::vector<NzbgetRsListing> raw = Listings(results);
	std::vector<size_t> indices(raw.size()), sizes(raw.size());
	size_t count = 0;
	nzbget_rs_posting_group(raw.data(), raw.size(), indices.data(), sizes.data(), &count);
	return Groups(results, indices, sizes, count);
}

bool Posting::ListingMismatch(const Newznab::Result& listing, long long nzbBytes)
{
	return nzbget_rs_posting_listing_mismatch((int64_t)listing.size, (int64_t)nzbBytes) != 0;
}

std::vector<Posting::Group> Posting::OrderPostings(const std::vector<Newznab::Result>& candidates,
	long long pickBytes, int maxPostings)
{
	std::vector<NzbgetRsListing> raw = Listings(candidates);
	std::vector<size_t> indices(raw.size()), sizes(raw.size());
	size_t count = 0;
	nzbget_rs_posting_order(raw.data(), raw.size(), (int64_t)pickBytes, maxPostings, indices.data(), sizes.data(), &count);
	return Groups(candidates, indices, sizes, count);
}
