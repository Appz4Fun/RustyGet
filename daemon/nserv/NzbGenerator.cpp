/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "NzbGenerator.h"
#include "Util.h"
#include "FileSystem.h"
#include "Log.h"
#include "nzbget_rs.h"

void NzbGenerator::Execute()
{
	info("Generating nzbs for %s", *m_dataDir);

	DirBrowser dir(m_dataDir);
	while (const char* filename = dir.Next())
	{
		NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			*m_dataDir, PATH_SEPARATOR, filename);
		BString<1024> fullFilename;
		fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
		nzbget_rs_free(fullFnBuf);

		if (nzbget_rs_snh_nzbgen_is_nzb_file(filename))
		{
			// skip nzb-files
			continue;
		}

		GenerateNzb(fullFilename);
	}

	info("Nzb generation finished");
}

void NzbGenerator::GenerateNzb(const char* path)
{
	NzbgetRsBuf nzbFnBuf = nzbget_rs_dqh_nzbgen_format_nzb_output_path(
		*m_dataDir, PATH_SEPARATOR, FileSystem::BaseFileName(path));
	BString<1024> nzbFilename;
	nzbFilename.Set(nzbFnBuf.data ? nzbFnBuf.data : "", (int)nzbFnBuf.len);
	nzbget_rs_free(nzbFnBuf);

	if (FileSystem::FileExists(nzbFilename))
	{
		return;
	}

	info("Generating nzb for %s", FileSystem::BaseFileName(path));

	DiskFile outfile;
	if (!outfile.Open(nzbFilename, DiskFile::omWrite))
	{
		error("Could not create file %s", *nzbFilename);
		return;
	}

	outfile.Print("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
	outfile.Print("<!DOCTYPE nzb PUBLIC \"-//newzBin//DTD NZB 1.0//EN\" \"http://www.newzbin.com/DTD/nzb/nzb-1.0.dtd\">\n");
	outfile.Print("<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n");

	bool isDir = FileSystem::DirectoryExists(path);
	if (isDir)
	{
		AppendDir(outfile, path);
	}
	else
	{
		AppendFile(outfile, path, nullptr);
	}

	outfile.Print("</nzb>\n");

	outfile.Close();
}

void NzbGenerator::AppendDir(DiskFile& outfile, const char* path)
{
	DirBrowser dir(path);
	while (const char* filename = dir.Next())
	{
		NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			path, PATH_SEPARATOR, filename);
		BString<1024> fullFilename;
		fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
		nzbget_rs_free(fullFnBuf);

		bool isDir = FileSystem::DirectoryExists(fullFilename);
		if (!isDir)
		{
			AppendFile(outfile, fullFilename, FileSystem::BaseFileName(path));
		}
	}
}

void NzbGenerator::AppendFile(DiskFile& outfile, const char* filename, const char* relativePath)
{
	detail("Processing %s", FileSystem::BaseFileName(filename));

	int64 fileSize = FileSystem::FileSize(filename);
	time_t timestamp = Util::CurrentTime();

	NzbgetRsBuf buf = nzbget_rs_nserv_format_nzb_file(
		fileSize, m_segmentSize, (int)timestamp, FileSystem::BaseFileName(filename), relativePath);
	if (buf.data)
	{
		outfile.Write(buf.data, buf.len);
	}
	nzbget_rs_free(buf);
}
