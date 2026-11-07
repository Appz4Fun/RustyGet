/*
 *  This file is part of RustyGet.
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
 *  along with this program. If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef POST_DOWNLOAD_RENAMER_H
#define POST_DOWNLOAD_RENAMER_H

#include <new>
#include <string>
#include "Thread.h"
#include "ScriptController.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "Log.h"
#include "nzbget_rs.h"

namespace PostDownloadRenamer
{
	class Controller final : public Thread, public ScriptController
	{
	public:
		static void StartJob(PostInfo* postInfo)
		{
			Controller* controller = new (std::nothrow) Controller();

			if (!controller)
			{
				error("Failed to allocate memory for PostDownloadRenamer::Controller");
				return;
			}

			controller->m_postInfo = postInfo;
			controller->SetAutoDestroy(false);

			postInfo->SetPostThread(controller);

			controller->Start();
		}

		void Run() override
		{
			std::string metaName;
			std::string nzbName;
			std::string dstDir;
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
				metaName = nzbInfo->GetMetaName();
				nzbName = nzbInfo->GetName() ? nzbInfo->GetName() : "";
				dstDir = nzbInfo->GetDestDir() ? nzbInfo->GetDestDir() : "";
			}

			int status = nzbget_rs_post_renamer_run(
				0,
				metaName.c_str(),
				nzbName.c_str(),
				dstDir.c_str(),
				g_Options->GetRenameIgnoreExt(),
				this,
				+[](void* rawCtx, const char* infoName) {
					static_cast<Controller*>(rawCtx)->SetInfoName(infoName ? infoName : "");
				},
				+[](void* rawCtx) -> int {
					return static_cast<Controller*>(rawCtx)->IsStopped() ? 1 : 0;
				},
				+[](void* rawCtx, int kind, const char* msg) {
					auto* self = static_cast<Controller*>(rawCtx);
					Message::EKind msgKind = kind == 1
						? Message::mkWarning
						: kind == 2
							? Message::mkError
							: Message::mkInfo;
					self->PrintMessage(msgKind, "%s", msg ? msg : "");
				},
				+[](void* rawCtx, const char* oldName, const char* newName) {
					auto* self = static_cast<Controller*>(rawCtx);
					GuardedDownloadQueue guard = DownloadQueue::Guard();
					if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
					{
						self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
							oldName ? oldName : "", newName ? newName : "");
					}
				});

			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetPostDownloadRenamingStatus(
				static_cast<NzbInfo::PostDownloadRenamingStatus>(status));
			m_postInfo->SetWorking(false);
		}

	protected:
		void AddMessage(Message::EKind kind, const char* text) override
		{
			m_postInfo->GetNzbInfo()->AddMessage(kind, text);
		}

	private:
		PostInfo* m_postInfo = nullptr;
	};
}

#endif
