#ifndef NZBGET_TLS_CIPHER_BACKEND_H
#define NZBGET_TLS_CIPHER_BACKEND_H
#include <string>
#include "nzbget_rs.h"
namespace TlsCipherBackend
{
	bool Available();
	std::string Start();
	std::string Status(const char* id);
	std::string Cancel(const char* id);
	void Shutdown();
	// Owned callback table; caller transfers it to Rust or invokes destroy once.
	NzbgetTlsBackend Create();
}
#endif
