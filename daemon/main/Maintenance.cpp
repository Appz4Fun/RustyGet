/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "Log.h"
#include "FileSystem.h"
#include "Maintenance.h"
#include "Options.h"
#include "nzbget_rs.h"

extern void ExitProc();
extern int g_ArgumentCount;
extern char* (*g_Arguments)[];


#ifndef DISABLE_TLS
class Signature
{
public:
	Signature(const char* inFilename, const char* sigFilename, const char* pubKeyFilename);
	~Signature();
	bool Verify();

private:
	const char* m_inFilename;
	const char* m_sigFilename;
	const char* m_pubKeyFilename;
	uchar m_inHash[SHA256_DIGEST_LENGTH];
	uchar m_signature[256];
	RSA* m_pubKey;

	bool ReadSignature();
	bool ComputeInHash();
	bool ReadPubKey();
};
#endif


Maintenance::~Maintenance()
{
	bool waitScript = false;
	{
		Guard guard(m_controllerMutex);
		if (m_updateScriptController)
		{
			m_updateScriptController->Detach();
			waitScript = true;
		}
	}

	if (waitScript)
	{
		while (m_updateScriptController)
		{
			Util::Sleep(20);
		}
	}
}

void Maintenance::ResetUpdateController()
{
	Guard guard(m_controllerMutex);
	m_updateScriptController = nullptr;
}

void Maintenance::AddMessage(Message::EKind kind, time_t time, const char * text)
{
	if (time == 0)
	{
		time = Util::CurrentTime();
	}

	Guard guard(m_logMutex);
	m_messages.emplace_back(++m_idMessageGen, kind, time, text);
}

bool Maintenance::StartUpdate(EBranch branch)
{
	// held to the end: two requests at once both found no script running and
	// started two (and lost track of the first)
	Guard guard(m_controllerMutex);

	if (m_updateScriptController)
	{
		error("Could not start update-script: update-script is already running");
		return false;
	}

	if (!ReadPackageInfoStr("install-script", m_updateScript))
	{
		return false;
	}

	// make absolute path
	{
		NzbgetRsBuf scriptPathBuf = nzbget_rs_dqh_maintenance_resolve_script_path(
			g_Options->GetAppDir(), PATH_SEPARATOR, *m_updateScript);
		m_updateScript.Set(scriptPathBuf.data ? scriptPathBuf.data : "", (int)scriptPathBuf.len);
		nzbget_rs_free(scriptPathBuf);
	}

	{
		Guard logGuard(m_logMutex);
		m_messages.clear();
	}

	m_updateScriptController = new UpdateScriptController();
	m_updateScriptController->SetArgs({*m_updateScript});
	m_updateScriptController->SetBranch(branch);
	m_updateScriptController->SetAutoDestroy(true);

	m_updateScriptController->Start();

	return true;
}

bool Maintenance::CheckUpdates(CString& updateInfo)
{
	CString updateInfoScript;
	if (!ReadPackageInfoStr("update-info-script", updateInfoScript))
	{
		return false;
	}

	UpdateInfoScriptController::ExecuteScript(updateInfoScript, updateInfo);

	return updateInfo.Length() > 0;
}

bool Maintenance::ReadPackageInfoStr(const char* key, CString& value)
{
	BString<1024> fileName;
	{
		NzbgetRsBuf fnBuf = nzbget_rs_dqh_maintenance_format_package_info_path(
			g_Options->GetWebDir(), PATH_SEPARATOR);
		fileName.Set(fnBuf.data ? fnBuf.data : "", (int)fnBuf.len);
		nzbget_rs_free(fnBuf);
	}

	CharBuffer packageInfo;
	if (!FileSystem::LoadFileIntoBuffer(fileName, packageInfo, true))
	{
		error("Could not load file %s", *fileName);
		return false;
	}

	size_t valStart = 0, valLen = 0;
	if (!nzbget_rs_feh_maintenance_extract_package_info_str(
		packageInfo, key, sizeof(fileName), &valStart, &valLen))
	{
		error("Could not parse file %s", *fileName);
		return false;
	}
	const char* p = (const char*)packageInfo + valStart;
	size_t len = valLen;

	value.Set(len > 0 ? p : "", (int)len);

	WebUtil::JsonDecode(value);

	return true;
}

bool Maintenance::VerifySignature(const char* inFilename, const char* sigFilename, const char* pubKeyFilename)
{
#ifndef DISABLE_TLS
	Signature signature(inFilename, sigFilename, pubKeyFilename);
	return signature.Verify();
#else
	return false;
#endif
}

void UpdateScriptController::Run()
{
	// the update-script should not be automatically terminated when the program quits
	UnregisterRunningScript();

	m_prefixLen = 0;
	PrintMessage(Message::mkInfo, "Executing update-script %s", GetScript());

	{
		NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(
			0, FileSystem::BaseFileName(GetScript()));
		SetInfoName(infoBuf.data ? infoBuf.data : "");
		nzbget_rs_free(infoBuf);
	}

	SetEnvVar("NZBUP_BRANCH", nzbget_rs_dqh_maintenance_branch_name((int)m_branch));

	SetEnvVar("NZBUP_RUNMODE", nzbget_rs_dqh_maintenance_run_mode_name(g_Options->GetDaemonMode() ? 1 : 0));

	for (int i = 0; i < g_ArgumentCount; i++)
	{
		NzbgetRsBuf envBuf = nzbget_rs_dqh_maintenance_format_cmdline_env_name(i);
		SetEnvVar(envBuf.data ? envBuf.data : "", (*g_Arguments)[i]);
		nzbget_rs_free(envBuf);
	}

	int pid = (int)getpid();

	{
		NzbgetRsBuf pidBuf = nzbget_rs_dqh_conn_format_port_str(pid);
		SetEnvVar("NZBUP_PROCESSID", pidBuf.data ? pidBuf.data : "");
		nzbget_rs_free(pidBuf);
	}

	const char* baseScript = FileSystem::BaseFileName(GetScript());
	size_t prefixLen = 0;
	{
		NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
			baseScript, 99, &prefixLen);
		SetLogPrefix(lpBuf.data ? lpBuf.data : "");
		nzbget_rs_free(lpBuf);
	}
	m_prefixLen = (int)prefixLen; // 2 = strlen(": ");

	Execute();

	g_Maintenance->ResetUpdateController();
}

void UpdateScriptController::AddMessage(Message::EKind kind, const char* text)
{
	text = text + m_prefixLen;

	int msgKind = nzbget_rs_feh_maintenance_classify_update_message(text);
	if (msgKind != 0)
	{
		debug("Command %s detected", text + 6);
		if (msgKind == 1)
		{
			Detach();
			ExitProc();
		}
		else
		{
			error("Invalid command \"%s\" received", text);
		}
	}
	else
	{
		g_Maintenance->AddMessage(kind, Util::CurrentTime(), text);
		ScriptController::AddMessage(kind, text);
	}
}

void UpdateInfoScriptController::ExecuteScript(const char* script, CString& updateInfo)
{
	detail("Executing update-info-script %s", FileSystem::BaseFileName(script));

	UpdateInfoScriptController scriptController;
	scriptController.SetArgs({script});

	const char* baseScript = FileSystem::BaseFileName(script);
	{
		NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(1, baseScript);
		scriptController.SetInfoName(infoBuf.data ? infoBuf.data : "");
		nzbget_rs_free(infoBuf);
	}

	size_t prefixLen = 0;
	{
		NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
			baseScript, 1023, &prefixLen);
		scriptController.SetLogPrefix(lpBuf.data ? lpBuf.data : "");
		nzbget_rs_free(lpBuf);
	}
	scriptController.m_prefixLen = (int)prefixLen; // 2 = strlen(": ");

	scriptController.Execute();

	if (!scriptController.m_updateInfo.Empty())
	{
		updateInfo = scriptController.m_updateInfo;
	}
}

void UpdateInfoScriptController::AddMessage(Message::EKind kind, const char* text)
{
	text = text + m_prefixLen;

	int infoKind = nzbget_rs_feh_maintenance_classify_update_info_message(text);
	if (infoKind != 0)
	{
		debug("Command %s detected", text + 6);
		if (infoKind == 1)
		{
			m_updateInfo.Append(text + 6 + 12);
		}
		else
		{
			error("Invalid command \"%s\" received from %s", text, GetInfoName());
		}
	}
	else
	{
		ScriptController::AddMessage(kind, text);
	}
}

#ifndef DISABLE_TLS
Signature::Signature(const char *inFilename, const char *sigFilename, const char *pubKeyFilename)
	: m_inHash{}
	, m_signature{}
{
	m_inFilename = inFilename;
	m_sigFilename = sigFilename;
	m_pubKeyFilename = pubKeyFilename;
	m_pubKey = nullptr;
}

Signature::~Signature()
{
	RSA_free(m_pubKey);
}

// Calculate SHA-256 for input file (m_inFilename)
bool Signature::ComputeInHash()
{
	DiskFile infile;
	if (!infile.Open(m_inFilename, DiskFile::omRead))
	{
		return false;
	}
	SHA256_CTX sha256;
	SHA256_Init(&sha256);
	CharBuffer buffer(32*1024);
	while(int bytesRead = (int)infile.Read(buffer, buffer.Size()))
	{
		SHA256_Update(&sha256, buffer, bytesRead);
	}
	SHA256_Final(m_inHash, &sha256);
	infile.Close();
	return true;
}

// Read signature from file (m_sigFilename) into memory
bool Signature::ReadSignature()
{
	BString<1024> sigTitle;
	{
		NzbgetRsBuf sigBuf = nzbget_rs_dqh_maintenance_format_sig_title(
			FileSystem::BaseFileName(m_inFilename));
		sigTitle.Set(sigBuf.data ? sigBuf.data : "", (int)sigBuf.len);
		nzbget_rs_free(sigBuf);
	}

	DiskFile infile;
	if (!infile.Open(m_sigFilename, DiskFile::omRead))
	{
		return false;
	}

	bool ok = false;
	char buf[1024];
	while (infile.ReadLine(buf, sizeof(buf) - 1))
	{
		size_t hexSigOffset = 0;
		if (nzbget_rs_dqh_maintenance_match_sig_line_prefix(buf, sigTitle, &hexSigOffset) != 0)
		{
			char* hexSig = buf + hexSigOffset;
			ok = nzbget_rs_feh_maintenance_decode_hex_signature(hexSig, m_signature) != 0;

			break;
		}
	}

	infile.Close();
	return ok;
}

// Read public key from file (m_szPubKeyFilename) into memory
bool Signature::ReadPubKey()
{
	CharBuffer keybuf;
	if (!FileSystem::LoadFileIntoBuffer(m_pubKeyFilename, keybuf, false))
	{
		return false;
	}
	BIO* mem = BIO_new_mem_buf(keybuf, keybuf.Size());
	m_pubKey = PEM_read_bio_RSA_PUBKEY(mem, nullptr, nullptr, nullptr);
	BIO_free(mem);
	return m_pubKey != nullptr;
}

bool Signature::Verify()
{
	return ComputeInHash() && ReadSignature() && ReadPubKey() &&
		RSA_verify(NID_sha256, m_inHash, sizeof(m_inHash), m_signature, sizeof(m_signature), m_pubKey) == 1;
}
#endif
