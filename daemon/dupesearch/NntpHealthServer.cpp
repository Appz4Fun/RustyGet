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

#include <algorithm>
#include <map>
#include <set>
#include "NntpHealthServer.h"
#include "ArticleFetcher.h"
#include "HttpGet.h"
#include "NewsServer.h"
#include "NntpConnection.h"
#include "ServerPool.h"
#include "Util.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

namespace
{

// a message-id as NNTP commands take it: in angle brackets (RFC 3977 3.6); nzb-files
// list it without them
std::string Bracketed(const std::string& messageId)
{
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_snh_nntp_health_is_bracketed(messageId.c_str(), messageId.size()))
	{
		return messageId;
	}
	return "<" + messageId + ">";
#else
	if (!messageId.empty() && messageId.front() == '<' && messageId.back() == '>')
	{
		return messageId;
	}
	return "<" + messageId + ">";
#endif
}

}

using Answer = DonorHealth::Answer;

int NntpHealthServer::CapFor(int connections)
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_snh_nntp_health_cap_for(connections, MaxCap);
#else
	return std::max(1, std::min(MaxCap, connections / 2));
#endif
}

DonorHealth::ServerList NntpHealthServer::Servers()
{
	static std::mutex mutex;
	static std::map<int, std::shared_ptr<NntpHealthServer>> known;

	std::lock_guard<std::mutex> guard(mutex);
	DonorHealth::ServerList list;
	std::set<int> groups;
	for (NewsServer* server : g_ServerPool->GetServers())
	{
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_din_nntp_health_is_server_eligible(server->GetActive() ? 1 : 0, server->GetMaxConnections()) == 0)
		{
			continue;
		}
#else
		if (!server->GetActive() || server->GetMaxConnections() == 0)
		{
			continue;
		}
#endif
		if (server->GetGroup() > 0 && !groups.insert(server->GetGroup()).second)
		{
			continue;
		}
		std::shared_ptr<NntpHealthServer>& entry = known[server->GetId()];
#ifdef NZBGET_USE_RUST
		int wantedCap = CapFor(server->GetMaxConnections());
		if (nzbget_rs_din_nntp_health_needs_entry_recreate(entry ? 1 : 0, entry ? entry->Cap() : 0, wantedCap) != 0)
		{
			entry = std::make_shared<NntpHealthServer>(server->GetId(), wantedCap);
		}
#else
		if (!entry || entry->Cap() != CapFor(server->GetMaxConnections()))
		{
			entry = std::make_shared<NntpHealthServer>(server->GetId(), CapFor(server->GetMaxConnections()));
		}
#endif
		list.push_back(entry);
	}
	return list;
}

std::vector<Answer> NntpHealthServer::Exchange(const std::vector<DonorHealth::Request>& batch)
{
	NewsServer* server = g_ServerPool->GetServerById(m_serverId);
	if (!server)
	{
		throw DonorHealth::ExchangeError();
	}

	// a connection of the pool: it stays the server's, only borrowed
	NntpConnection* connection = nullptr;
	time_t waitStart = Util::CurrentTime();
#ifdef NZBGET_USE_RUST
	while (nzbget_rs_din_nntp_health_should_keep_waiting_for_conn(
		connection ? 1 : 0, HttpGet::Stopped() ? 1 : 0,
		(int64_t)Util::CurrentTime(), (int64_t)waitStart, ConnectionWaitSec) != 0)
#else
	while (!connection && !HttpGet::Stopped() && Util::CurrentTime() - waitStart <= ConnectionWaitSec)
#endif
	{
		connection = g_ServerPool->GetConnection(server->GetNormLevel(), server, nullptr);
		if (!connection)
		{
			Util::Sleep(20);
		}
	}
	if (!connection)
	{
		DonorHealth::ExchangeError busy;
		busy.busy = true;
		throw busy;
	}

	struct Release
	{
		NntpConnection* connection;
		bool broken = false;
		~Release()
		{
			if (broken)
			{
				connection->Disconnect();
			}
			g_ServerPool->FreeConnection(connection, true);
		}
	} release{ connection };

	if (!connection->Connect())
	{
		release.broken = true;
		throw DonorHealth::ExchangeError();
	}

	if (server->GetJoinGroup() && batch.front().groups)
	{
		const char* joined = nullptr;
		for (const std::string& group : *batch.front().groups)
		{
			joined = connection->JoinGroup(group.c_str());
#ifdef NZBGET_USE_RUST
			if (nzbget_rs_din_nntp_health_is_join_group_ok(joined) != 0)
#else
			if (joined && !strncmp(joined, "2", 1))
#endif
			{
				break;
			}
		}
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_din_nntp_health_is_join_group_ok(joined) == 0)
#else
		if (!joined || strncmp(joined, "2", 1))
#endif
		{
			release.broken = !joined;
			throw DonorHealth::ExchangeError();
		}
	}

	// all STATs in one write, then their replies in order (RFC 3977 section 3.5)
	std::string commands;
	for (const DonorHealth::Request& request : batch)
	{
		commands += "STAT " + Bracketed(request.messageId) + "\r\n";
	}
	if (!connection->Send(commands.c_str(), (int)commands.size()))
	{
		release.broken = true;
		throw DonorHealth::ExchangeError();
	}

	// 223 present; 430 definitively missing; any other reply (some providers
	// say 451 for a missing article) is an answer on a healthy connection that
	// proves nothing
	std::vector<Answer> answers;
	for (size_t i = 0; i < batch.size(); i++)
	{
        // Discovery can grow the pipeline on a fast pick before checking a slow
        // donor. On shutdown abandon unread replies instead of draining it.
        if (HttpGet::Stopped() || DonorHealth::Stopping())
        {
            release.broken = true;
            DonorHealth::ExchangeError error;
            error.answers = answers;
            throw error;
        }
		char line[1024];
		int length = 0;
		if (!connection->ReadLine(line, sizeof(line), &length))
		{
			release.broken = true;
			DonorHealth::ExchangeError error;
			error.answers = answers;
			throw error;
		}
#ifdef NZBGET_USE_RUST
		int statReply = nzbget_rs_snh_nntp_health_classify_stat_reply(line);
		answers.push_back(statReply == 0 ? Answer::Present : statReply == 1 ? Answer::Missing : Answer::Error);
#else
		answers.push_back(!strncmp(line, "223", 3) ? Answer::Present : !strncmp(line, "430", 3) ? Answer::Missing : Answer::Error);
#endif
	}

	// BODY for the articles that have a gate: one server at a time, verified
	for (size_t i = 0; i < batch.size(); i++)
	{
        if (HttpGet::Stopped() || DonorHealth::Stopping())
        {
            release.broken = true;
            DonorHealth::ExchangeError error;
            error.answers = answers;
            for (size_t j = i; j < batch.size(); j++)
#ifdef NZBGET_USE_RUST
                if (nzbget_rs_din_nntp_health_should_reset_gated_present_to_error(
                    batch[j].gate ? 1 : 0, (int)error.answers[j], (int)Answer::Present) != 0)
                    error.answers[j] = Answer::Error;
#else
                if (batch[j].gate && error.answers[j] == Answer::Present) error.answers[j] = Answer::Error;
#endif
            throw error;
        }
		if (!batch[i].gate || answers[i] != Answer::Present)
		{
			continue;
		}
		try
		{
			answers[i] = BodyOnce(batch[i].gate, [&]() -> Answer
				{
					std::vector<CString> groups;
					if (batch[i].groups)
					{
						for (const std::string& group : *batch[i].groups)
						{
							groups.emplace_back(group.c_str());
						}
					}
					ArticleFetcher fetcher;
					ArticleFetcher::FetchedArticle article = fetcher.FetchFromConnection(connection, Bracketed(batch[i].messageId).c_str(), groups);
#ifdef NZBGET_USE_RUST
					int throwError = 0;
					int classified = nzbget_rs_din_nntp_health_classify_body_fetch_result(
						article.Success ? 1 : 0, article.Transient ? 1 : 0,
						(int)Answer::Present, (int)Answer::BodyBad, &throwError);
					if (throwError != 0)
					{
						throw DonorHealth::ExchangeError();
					}
					return (Answer)classified;
#else
					if (article.Success)
					{
						return Answer::Present;
					}
					if (article.Transient)
					{
						// the connection broke mid-body
						throw DonorHealth::ExchangeError();
					}
					return Answer::BodyBad;
#endif
				});
		}
		catch (DonorHealth::ExchangeError&)
		{
			// the STAT answers already read stay; the unread bodies are unknown
			release.broken = true;
			DonorHealth::ExchangeError error;
			error.answers = answers;
			error.answers[i] = Answer::Error;
			for (size_t j = i + 1; j < batch.size(); j++)
			{
#ifdef NZBGET_USE_RUST
				if (nzbget_rs_din_nntp_health_should_reset_gated_present_to_error(
					batch[j].gate ? 1 : 0, (int)answers[j], (int)Answer::Present) != 0)
#else
				if (batch[j].gate && answers[j] == Answer::Present)
#endif
				{
					error.answers[j] = Answer::Error;
				}
			}
			throw error;
		}
	}
	return answers;
}
