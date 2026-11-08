/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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


#ifndef CONNECTION_H
#define CONNECTION_H

#include <atomic>
#include <string>
#include <memory>
#include <vector>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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

#ifndef TLSSOCKET_H
#define TLSSOCKET_H

#include <chrono>
#include <functional>
#include <cstdint>
#include "nzbget_rs.h"

struct SocketDeadline
{
	std::chrono::steady_clock::time_point end;
	std::function<bool()> cancelled;
	bool Stopped() const { return std::chrono::steady_clock::now() >= end || (cancelled && cancelled()); }
	bool Wait(SOCKET socket, bool read, bool write) const
	{
		return nzbget_rs_socket_deadline_wait(
			(int)socket,
			read ? 1 : 0,
			write ? 1 : 0,
			const_cast<SocketDeadline*>(this),
			+[](void* ctx) noexcept -> int64_t
			{
				const auto* self = static_cast<const SocketDeadline*>(ctx);
				if (self->Stopped()) return -1;
				return std::chrono::duration_cast<std::chrono::milliseconds>(
					self->end - std::chrono::steady_clock::now()).count();
			}) != 0;
	}
};

#ifndef DISABLE_TLS

#include <string_view>
#include <openssl/sslerr.h>
#include "Util.h"
#include "Util.h"
#include "Util.h"

class TlsSocket
{
public:
	TlsSocket(
		SOCKET socket,
		bool isClient,
		const char* host,
		const char* certFile,
		const char* keyFile,
		const char* cipher,
		int certVerifLevel)
		: m_socket(socket)
		, m_host(host ? host : "")
		, m_certFile(certFile ? certFile : "")
		, m_keyFile(keyFile ? keyFile : "")
		, m_cipher(cipher ? cipher : "")
		, m_isClient(isClient)
		, m_retCode(0)
		, m_certVerifLevel(certVerifLevel)
		, m_context{ nullptr, &SSL_CTX_free }
		, m_session{ nullptr, &SSL_free } {}

	virtual ~TlsSocket()
	{
		Close();
		ERR_clear_error();
	}

	static void Init()
	{
		debug("Initializing TLS library");

		if (OPENSSL_init_ssl(0, nullptr) == 0)
		{
			error("Could not initialize the OpenSSL library");
		}
	}

	static void InitOptions(const char* certStore)
	{
		if (Util::EmptyStr(certStore)) 
			return;

		InitX509Store(certStore);
	}

	static void Final()
	{
		m_X509Store.reset();
		OPENSSL_cleanup();
	}

	bool Start()
	{
		m_context.reset(SSL_CTX_new(TLS_method()));

		if (!m_context)
		{
			ReportError("Could not create TLS context", false);
			return false;
		}

		if (nzbget_rs_dqh_tls_has_cert_and_key_files(
			m_certFile.empty() ? 1 : 0, m_keyFile.empty() ? 1 : 0) != 0)
		{
			if (!SSL_CTX_use_certificate_chain_file(m_context.get(), m_certFile.c_str()))
			{
				ReportError("Could not load certificate file", false);
				Close();
				return false;
			}
			if (!SSL_CTX_use_PrivateKey_file(m_context.get(), m_keyFile.c_str(), SSL_FILETYPE_PEM))
			{
				ReportError("Could not load key file", false);
				Close();
				return false;
			}
		}

		if (nzbget_rs_dqh_tls_should_verify_client_x509_store(
			m_isClient ? 1 : 0, m_X509Store ? 1 : 0) != 0)
		{
			SSL_CTX_set1_cert_store(m_context.get(), m_X509Store.get());

			if (nzbget_rs_ntw_tls_should_verify_peer(
				1, (int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvNone) != 0)
			{
				SSL_CTX_set_verify(m_context.get(), SSL_VERIFY_PEER, nullptr);
			}
			else
			{
				SSL_CTX_set_verify(m_context.get(), SSL_VERIFY_NONE, nullptr);
			}
		}

		m_session.reset(SSL_new(m_context.get()));
		if (!m_session)
		{
			ReportError("Could not create TLS session", false);
			Close();
			return false;
		}

		if (!SetCipherSuite(m_cipher))
		{
			ReportError("Could not set cipher suite for SSL/TLS connection", false);
			Close();
			return false;
		}

		if (nzbget_rs_dqh_tls_should_set_sni_hostname(
			m_isClient ? 1 : 0, m_host.empty() ? 1 : 0) != 0 &&
			!SSL_set_tlsext_host_name(m_session.get(), m_host.c_str()))
		{
			ReportError("Could not set host name for TLS");
			Close();
			return false;
		}

		if (!SSL_set_fd(m_session.get(), static_cast<int>(m_socket)))
		{
			ReportError("Could not set the file descriptor for TLS");
			Close();
			return false;
		}

		int ec;
		do
		{
			if (m_deadline && m_deadline->Stopped()) { Close(); return false; }
			ec = m_isClient ? SSL_connect(m_session.get()) : SSL_accept(m_session.get());
		} while (ec < 1 && RetryIo(ec));
		bool shouldVerify = nzbget_rs_ntw_tls_should_verify_peer(
			m_X509Store ? 1 : 0, (int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvNone) != 0;
		if (ec < 1)
		{
			if (m_deadline)
			{
				unsigned long code = ERR_peek_last_error();
				int reason = ERR_GET_REASON(code);
				if (nzbget_rs_ntw_tls_is_explicit_handshake_rejection(
					ERR_GET_LIB(code), ERR_LIB_SSL, reason,
					SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE,
					SSL_R_TLSV1_ALERT_PROTOCOL_VERSION,
					SSL_R_NO_SHARED_CIPHER,
					SSL_R_UNSUPPORTED_PROTOCOL) != 0)
					m_probeFailure = 0; // Explicit rejection of this single-suite handshake.
			}
			if (nzbget_rs_dqh_tls_should_check_verify_result_on_handshake_fail(
				m_isClient ? 1 : 0, shouldVerify ? 1 : 0) != 0)
			{
				long verifyRes = SSL_get_verify_result(m_session.get());
				if (verifyRes != X509_V_OK)
				{
					m_probeFailure = -4;
					NzbgetRsBuf verifyErrBuf = nzbget_rs_dqh_tls_format_cert_verify_err(
						m_host.c_str(), X509_verify_cert_error_string(verifyRes));
					PrintError(verifyErrBuf.data ? verifyErrBuf.data : "");
					nzbget_rs_free(verifyErrBuf);
					Close();
					return false;
				}
			}

			NzbgetRsBuf hsErrBuf = nzbget_rs_dqh_tls_format_handshake_err(m_host.c_str());
			ReportError(hsErrBuf.data ? hsErrBuf.data : "");
			nzbget_rs_free(hsErrBuf);
			Close();
			return false;
		}

		if (nzbget_rs_dqh_tls_should_validate_client_cert(
			m_isClient ? 1 : 0, shouldVerify ? 1 : 0) != 0 && !ValidateCert())
		{
			m_probeFailure = -4;
			Close();
			return false;
		}

		const SSL_CIPHER* currentCipher = SSL_get_current_cipher(m_session.get());
		if (currentCipher != nullptr)
		{
			debug("TLS connection established: %s using %s", SSL_get_version(m_session.get()), SSL_CIPHER_get_name(currentCipher));
		}

		m_connected = true;
		return true;
	}

	void Close()
	{
		if (nzbget_rs_dqh_tls_should_send_close_notify(
			m_session ? 1 : 0, m_connected ? 1 : 0, m_deadline ? 1 : 0) != 0)
		{
			if (SSL_shutdown(m_session.get()) == 0)
			{
				// The bidirectional shutdown is not yet complete. Call SSL_shutdown again.
				SSL_shutdown(m_session.get());
			}
		}

		m_context.reset();
		m_session.reset();
	}

	int Send(const char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_write(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(nzbget_rs_dqh_tls_select_io_err_msg(1, ERR_peek_error() == 0 ? 1 : 0));
			return -1;
		}

		return m_retCode;
	}

	int Recv(char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_read(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(nzbget_rs_dqh_tls_select_io_err_msg(0, ERR_peek_error() == 0 ? 1 : 0));
			return -1;
		}

		return m_retCode;
	}

	void SetSuppressErrors(bool suppressErrors) { m_suppressErrors = suppressErrors; }
	const char* GetNegotiatedCipher() const { return m_session ? SSL_get_cipher_name(m_session.get()) : ""; }
	int GetNegotiatedVersion() const { return m_session ? SSL_version(m_session.get()) : 0; }
	static bool HasCertificateStore() { return static_cast<bool>(m_X509Store); }
	int GetProbeFailure() const { return m_probeFailure; }
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline) { m_deadline = std::move(deadline); }

protected:
	virtual void PrintError(const char* errMsg)
	{
		error("%s", errMsg);
	}

private:
	static void InitX509Store(std::string_view certStore)
	{
		m_X509Store.reset(X509_STORE_new());

		if (!m_X509Store)
		{
			error("Could not create certificate store");
			return;
		}

		if (!X509_STORE_load_locations(m_X509Store.get(), certStore.data(), nullptr))
		{
			error("Could not load certificate store location from %s", certStore.data());
			return;
		}
	}

	inline static OpenSSL::X509StorePtr m_X509Store{ nullptr, &X509_STORE_free };

	/**
	 * This function configures the allowed ciphers for both TLSv1.3 and
	 * older protocols from a single, combined string. OpenSSL
	 * will parse the string and apply the relevant ciphers to their
	 * respective protocol versions.
	 *
	 * * @note This implementation allows a mixed list, but TLSv1.3 ciphers will
	 * always be preferred during the handshake if the server supports them,
	 * regardless of their order in the string.
	 */
	bool SetCipherSuite(std::string_view cipher)
	{
		if (cipher.empty())
		{
			return true;
		}

		// A single modern suite is an exact pin. Without a protocol bound, setting
		// only a TLS 1.2 list leaves the default TLS 1.3 suites usable (and vice versa).
		// Empty values and custom lists retain their historical negotiation behavior.
		if (!nzbget_rs_dqh_tls_is_cipher_list(cipher.data(), cipher.size()))
		{
			const auto* ciphers = SSL_get_ciphers(m_session.get());
			for (int i = 0; i < sk_SSL_CIPHER_num(ciphers); ++i)
			{
				const SSL_CIPHER* candidate = sk_SSL_CIPHER_value(ciphers, i);
				const char* candName = SSL_CIPHER_get_name(candidate);
				if (cipher != candName) continue;
				bool tls13 = nzbget_rs_dqh_tls_is_tls13_version_str(SSL_CIPHER_get_version(candidate)) != 0;
				int auth = SSL_CIPHER_get_auth_nid(candidate), kx = SSL_CIPHER_get_kx_nid(candidate);
				unsigned flags = nzbget_rs_dqh_tls_compute_suite_flags(
					SSL_CIPHER_is_aead(candidate) ? 1 : 0,
					tls13 ? 1 : 0,
					(auth == NID_auth_rsa || auth == NID_auth_ecdsa) ? 1 : 0,
					(kx == NID_kx_ecdhe || kx == NID_kx_dhe) ? 1 : 0);
				unsigned bits = nzbget_rs_dqh_tls_resolve_cipher_bits(
					candName, (unsigned)SSL_CIPHER_get_bits(candidate, nullptr));
				if (!nzbget_rs_tls_cipher_pin(tls13 ? 13 : 12, bits, flags)) break;
				int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
				int minimum = SSL_get_min_proto_version(m_session.get());
				int maximum = SSL_get_max_proto_version(m_session.get());
				if (nzbget_rs_dqh_tls_is_proto_version_out_of_bounds(version, minimum, maximum) != 0) return false;
				if (!SSL_set_min_proto_version(m_session.get(), version) || !SSL_set_max_proto_version(m_session.get(), version)) return false;
				return tls13 ? SSL_set_ciphersuites(m_session.get(), m_cipher.c_str()) == 1 :
					SSL_set_cipher_list(m_session.get(), m_cipher.c_str()) == 1;
			}
		}

		bool tls13 = SSL_set_ciphersuites(m_session.get(), cipher.data());
		bool legacy = SSL_set_cipher_list(m_session.get(), cipher.data());

		if (!tls13)
			ERR_clear_error();

		return tls13 || legacy;
	}

	SOCKET m_socket;

	std::string m_host;
	std::string m_certFile;
	std::string m_keyFile;
	std::string m_cipher;

	bool m_isClient;
	bool m_suppressErrors = false;
	bool m_connected = false;
	int m_probeFailure = -1;

	int m_retCode;
	int m_certVerifLevel;

	OpenSSL::SSLCtxPtr m_context;
	OpenSSL::SSLSessionPtr m_session;
	std::shared_ptr<const SocketDeadline> m_deadline;

	bool RetryIo(int result)
	{
		if (!m_deadline) return false;
		int error = SSL_get_error(m_session.get(), result);
		int wantRead = 0;
		int wantWrite = 0;
		if (!nzbget_rs_ntw_tls_classify_want_io(
			error, SSL_ERROR_WANT_READ, SSL_ERROR_WANT_WRITE, &wantRead, &wantWrite))
			return false;
		return m_deadline->Wait(m_socket, wantRead != 0, wantWrite != 0);
	}

	void ReportError(const char* errMsg, bool suppressable = true)
	{
		auto errcode = ERR_get_error();
		do
		{
			char errstr[1024];
			ERR_error_string_n(errcode, errstr, sizeof(errstr));
			errstr[1024 - 1] = '\0';

			if (suppressable && m_suppressErrors)
			{
				NzbgetRsBuf formattedErr = nzbget_rs_dqh_tls_format_report_err(errMsg, errstr);
				debug("%s", formattedErr.data ? formattedErr.data : "");
				nzbget_rs_free(formattedErr);
			}
			else if (errcode != 0)
			{
				NzbgetRsBuf formattedErr = nzbget_rs_dqh_tls_format_report_err(errMsg, errstr);
				PrintError(formattedErr.data ? formattedErr.data : "");
				nzbget_rs_free(formattedErr);
			}
			else
			{
				PrintError(errMsg);
			}

			errcode = ERR_get_error();
		} while (errcode);
	}

	bool ValidateCert()
	{
		// verify a server certificate was presented during the negotiation
		OpenSSL::X509Ptr cert{ SSL_get_peer_certificate(m_session.get()), &X509_free };
		if (!cert)
		{
			NzbgetRsBuf verifyErrBuf = nzbget_rs_dqh_tls_format_cert_verify_err(
				m_host.c_str(), "no certificate provided by server");
			PrintError(verifyErrBuf.data ? verifyErrBuf.data : "");
			nzbget_rs_free(verifyErrBuf);
			return false;
		}

#ifdef HAVE_X509_CHECK_HOST
		// hostname verification
		bool shouldCheckHost = nzbget_rs_ntw_tls_should_check_host(
			(int)m_certVerifLevel, (int)Options::ECertVerifLevel::cvMinimal, m_host.empty() ? 1 : 0) != 0;
		if (shouldCheckHost && !X509_check_host(cert.get(), m_host.c_str(), m_host.size(), 0, nullptr))
		{
			const unsigned char* certHost = nullptr;
			// Find the position of the CN field in the Subject field of the certificate
			int common_name_loc = X509_NAME_get_index_by_NID(X509_get_subject_name(cert.get()), NID_commonName, -1);
			if (common_name_loc >= 0)
			{
				// Extract the CN field
				const X509_NAME_ENTRY* common_name_entry = X509_NAME_get_entry(X509_get_subject_name(cert.get()), common_name_loc);
				if (common_name_entry != nullptr)
				{
					// Convert the CN field to a C string
					const ASN1_STRING* common_name_asn1 = X509_NAME_ENTRY_get_data(common_name_entry);
					if (common_name_asn1 != nullptr)
					{
						certHost = ASN1_STRING_get0_data(common_name_asn1);
					}
				}
			}

			NzbgetRsBuf mismatchErrBuf = nzbget_rs_dqh_tls_format_hostname_mismatch_err(
				m_host.c_str(), reinterpret_cast<const char*>(certHost));
			PrintError(mismatchErrBuf.data ? mismatchErrBuf.data : "");
			nzbget_rs_free(mismatchErrBuf);
			return false;
		}
#endif

		return true;
	}
};

#endif

#ifndef SYSTEM_INFO_H
#define SYSTEM_INFO_H

#include <ostream>
#include <string>

namespace System
{
	inline const char* IP_SERVICE = "ip.nzbget.com";

	struct Network
	{
		std::string publicIP;
		std::string privateIP;
	};

	inline Network GetNetwork()
	{
		Network network;

#ifndef DISABLE_TLS
		int useTls = 1;
#else
		int useTls = 0;
#endif
#ifndef DISABLE_GZIP
		int includeGzip = 1;
#else
		int includeGzip = 0;
#endif
		NzbgetRsBuf pubBuf{};
		NzbgetRsBuf privBuf{};
		NzbgetRsBuf errBuf{};
		int ok = nzbget_rs_system_info_fetch_network(
			IP_SERVICE,
			useTls,
			includeGzip,
			Util::VersionRevision(),
			nullptr,
			[](void*, int fd, const char* host, const char* req, size_t reqLen, NzbgetRsBuf* outResp) -> int
			{
#ifndef DISABLE_TLS
				SSL_CTX* ctx = SSL_CTX_new(TLS_client_method());
				if (!ctx)
				{
					const char* msg = "Failed to create SSL_CTX";
					*outResp = nzbget_rs_alloc_buf(msg, strlen(msg));
					return 0;
				}
				SSL_CTX_set_default_verify_paths(ctx);
				SSL* ssl = SSL_new(ctx);
				if (!ssl)
				{
					SSL_CTX_free(ctx);
					const char* msg = "Failed to create SSL";
					*outResp = nzbget_rs_alloc_buf(msg, strlen(msg));
					return 0;
				}
				SSL_set_fd(ssl, fd);
				SSL_set_tlsext_host_name(ssl, host);
				if (SSL_connect(ssl) != 1)
				{
					SSL_free(ssl);
					SSL_CTX_free(ctx);
					OpenSSL::StopSSLThread();
					const char* msg = "TLS handshake failed";
					*outResp = nzbget_rs_alloc_buf(msg, strlen(msg));
					return 0;
				}

				size_t written = 0;
				while (written < reqLen)
				{
					int n = SSL_write(ssl, req + written, static_cast<int>(reqLen - written));
					if (n <= 0) break;
					written += static_cast<size_t>(n);
				}

				std::string rawResp;
				char chunk[4096];
				for (;;)
				{
					int n = SSL_read(ssl, chunk, sizeof(chunk));
					if (n <= 0) break;
					rawResp.append(chunk, static_cast<size_t>(n));
				}

				SSL_shutdown(ssl);
				SSL_free(ssl);
				SSL_CTX_free(ctx);
				OpenSSL::StopSSLThread();
				*outResp = nzbget_rs_alloc_buf(rawResp.data(), rawResp.size());
				return 1;
#else
				(void)fd;
				(void)host;
				(void)req;
				(void)reqLen;
				(void)outResp;
				return 0;
#endif
			},
			&pubBuf,
			&privBuf,
			&errBuf);
		if (ok)
		{
			network.publicIP.assign(pubBuf.data ? pubBuf.data : "", pubBuf.len);
			network.privateIP.assign(privBuf.data ? privBuf.data : "", privBuf.len);
		}
		else if (errBuf.data && errBuf.len > 0)
		{
			detail("Failed to get public and private IP: %s", errBuf.data);
		}
		nzbget_rs_free(pubBuf);
		nzbget_rs_free(privBuf);
		nzbget_rs_free(errBuf);

		return network;
	}

	class SystemInfo final
	{
	public:
		SystemInfo(std::function<Network()> networkFetcher = GetNetwork)
			: m_networkFetcher{std::move(networkFetcher)}
		{
#ifndef DISABLE_TLS
#ifdef LIBRESSL_VERSION_TEXT
			m_sslLibName = "LibreSSL";
			m_sslRawVersion = LIBRESSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#elif defined(OPENSSL_FULL_VERSION_STR)
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_FULL_VERSION_STR;
			m_sslExtractAfterSpace = false;
#else
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#endif
#endif
		}

		~SystemInfo() = default;

		Network GetNetworkInfo() const
		{
			return m_networkFetcher();
		}

		std::string Format(int mode, bool withNetwork) const
		{
			Network net = withNetwork ? GetNetworkInfo() : Network{};
			NzbgetRsBuf buf = nzbget_rs_system_info_format(
				mode,
				"",
				"",
				"",
				m_sslLibName.c_str(),
				m_sslRawVersion.c_str(),
				m_sslExtractAfterSpace ? 1 : 0,
				net.publicIP.data(), net.publicIP.size(),
				net.privateIP.data(), net.privateIP.size());
			std::string result(buf.data ? buf.data : "", buf.len);
			nzbget_rs_free(buf);
			return result;
		}

		friend std::ostream& operator<<(std::ostream& os, const SystemInfo& sysinfo)
		{
			std::string text = sysinfo.Format(0, false);
			if (!text.empty())
			{
				os.write(text.data(), static_cast<std::streamsize>(text.size()));
			}
			return os;
		}

	private:
		std::string m_sslLibName;
		std::string m_sslRawVersion;
		bool m_sslExtractAfterSpace = false;
		std::function<Network()> m_networkFetcher;
	};

	inline std::string ToJsonStr(const SystemInfo& sysInfo)
	{
		return sysInfo.Format(1, true);
	}

	inline std::string ToXmlStr(const SystemInfo& sysInfo)
	{
		return sysInfo.Format(2, true);
	}
}

extern System::SystemInfo* g_SystemInfo;

#endif
#endif


class Connection
{
public:
	enum EStatus
	{
		csConnected,
		csDisconnected,
		csListening,
		csCancelled,
		csBroken
	};

	enum EIPVersion
	{
		ipAuto = 0,
		ipV4 = 4,
		ipV6 = 6
	};

	Connection(const char* host, int port, bool tls);
	Connection(SOCKET socket, bool tls);
	virtual ~Connection();
	static void Init();
	static void Final();
	virtual bool Connect();
	virtual bool Disconnect();
	bool Bind();
	bool Send(const char* buffer, int size);
	bool Recv(char* buffer, int size);
	int TryRecv(char* buffer, int size);
	char* ReadLine(char* buffer, int size, int* bytesRead);
	void ReadBuffer(char** buffer, int *bufLen);
	int WriteLine(const char* buffer);
	std::unique_ptr<Connection> Accept();
	void Cancel();
	const char* GetHost() const { return m_host.c_str(); }
	int GetPort() { return m_port; }
	bool GetTls() { return m_tls; }
	const char* GetCipher() const { return m_cipher.c_str(); }
	void SetCipher(const char* cipher) { m_cipher = cipher ? cipher : ""; }
	void SetTimeout(int timeout) { m_timeout = timeout; }
	void SetResolvedAddresses(const std::vector<std::string>& addresses) { m_resolvedAddresses = addresses; }
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline) { m_deadline = std::move(deadline); }
#ifndef DISABLE_TLS
	const char* GetNegotiatedCipher() const { return m_tlsSocket ? m_tlsSocket->GetNegotiatedCipher() : ""; }
	int GetNegotiatedVersion() const { return m_tlsSocket ? m_tlsSocket->GetNegotiatedVersion() : 0; }
	int GetTlsProbeFailure() const { return m_tlsProbeFailure; }
#endif
	void SetIPVersion(EIPVersion ipVersion) { m_ipVersion = ipVersion; }
	EStatus GetStatus() { return m_status; }
	// the last Accept() failed for lack of descriptors or memory, not on the listener
	bool GetAcceptBusy() { return m_acceptBusy; }
	// the peer can't be relied on any more (a timeout mid-reply): Disconnect sends
	// nothing and waits for nothing
	void SetBroken() { m_status = csBroken; }
	void SetSuppressErrors(bool suppressErrors);
	bool GetSuppressErrors() { return m_suppressErrors; }
	const char* GetRemoteAddr();
	bool GetGracefull() { return m_gracefull; }
	void SetGracefull(bool gracefull) { m_gracefull = gracefull; }
	void SetForceClose(bool forceClose) { m_forceClose = forceClose; }
#ifndef DISABLE_TLS
	bool StartTls(bool isClient, const char* certFile, const char* keyFile);
	void SetCertVerifLevel(unsigned int level) { m_certVerifLevel = level; }
#endif
	int FetchTotalBytesRead();

protected:
	std::string m_host;
	std::string m_cipher;
	std::vector<std::string> m_resolvedAddresses;
	std::shared_ptr<const SocketDeadline> m_deadline;
	int m_port;
	bool m_tls;
	EIPVersion m_ipVersion = ipAuto;
	SOCKET m_socket = INVALID_SOCKET;
	CharBuffer m_readBuf;
	int m_bufAvail = 0;
	char* m_bufPtr = nullptr;
	std::atomic<EStatus> m_status{csDisconnected};
	int m_timeout = 60;
	bool m_suppressErrors = true;
	bool m_acceptBusy = false;
	BString<100> m_remoteAddr;
	int m_totalBytesRead = 0;
	bool m_gracefull = false;
	bool m_forceClose = false;
#ifndef DISABLE_TLS
	unsigned int m_certVerifLevel;
#endif

	struct SockAddr
	{
		int ai_family;
		int ai_socktype;
		int ai_protocol;
		bool operator==(const SockAddr& rhs) const
			{ return memcmp(this, &rhs, sizeof(SockAddr)) == 0; }
	};

#ifndef DISABLE_TLS
	std::unique_ptr<TlsSocket> m_tlsSocket;
	bool m_tlsError = false;
	int m_tlsProbeFailure = -1;
#endif
#ifndef HAVE_GETADDRINFO
#ifndef HAVE_GETHOSTBYNAME_R
	inline static std::mutex m_getHostByNameMutex;
#endif
#endif

	void ReportError(const char* msgPrefix, const char* msgArg, bool printErrCode, int errCode = 0,
		const char* errMsg = nullptr);
	virtual void PrintError(const char* errMsg);
	int GetLastNetworkError();
	bool DoConnect();
	void DoDisconnect();
	bool InitSocketOpts(SOCKET socket);
	bool ConnectWithTimeout(void* address, int address_len);
#ifndef HAVE_GETADDRINFO
	in_addr_t ResolveHostAddr(const char* host);
#endif
#ifndef DISABLE_TLS
	int recv(SOCKET s, char* buf, int len, int flags);
	int send(SOCKET s, const char* buf, int len, int flags);
	void CloseTls();
#endif
};

#include "nzbget_rs.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"

// sockets aren't inherited by the scripts nzbget starts: a script that outlived
// the program kept the control port listening, and copies of client sockets kept
// connections from closing
#ifdef SOCK_CLOEXEC
#define SOCKET_CLOEXEC SOCK_CLOEXEC
#else
#define SOCKET_CLOEXEC 0
#endif

// a socket is close-on-exec, so that the scripts nzbget starts don't inherit
// its connections and listeners. socket() takes the flag where SOCK_CLOEXEC
// exists, accept4() on Linux; elsewhere (macOS has neither) it is set afterwards
static inline void SetCloseOnExec(SOCKET socket)
{
	if (socket != INVALID_SOCKET)
	{
		fcntl(socket, F_SETFD, fcntl(socket, F_GETFD, 0) | FD_CLOEXEC);
	}
}

#include <poll.h>


static const int CONNECTION_READBUFFER_SIZE = 1024;
// past the data read into a buffer: a decoder writing into it (uuencoded lines) may
// write up to one line's worth more than it was given
static const int DECODE_SLACK = 128;

inline void closesocket_gracefully(SOCKET socket)
{
	char buf[1024];
	struct linger linger;

	// Set linger option to avoid socket hanging out after close. This prevent
	// ephemeral port exhaust problem under high QPS.
	linger.l_onoff = 1;
	linger.l_linger = 1;
	setsockopt(socket, SOL_SOCKET, SO_LINGER, (char *) &linger, sizeof(linger));

	// Send FIN to the client
	shutdown(socket, SHUT_WR);

	// Set non-blocking mode
	int flags;
	flags = fcntl(socket, F_GETFL, 0);
	fcntl(socket, F_SETFL, flags | O_NONBLOCK);

	// Read and discard pending incoming data. If we do not do that and close the
	// socket, the data in the send buffer may be discarded. This
	// behaviour is seen on Windows, when client keeps sending data
	// when server decides to close the connection; then when client
	// does recv() it gets no data back.
	int n;
	do {
		n = recv(socket, buf, sizeof(buf), 0);
	} while (n > 0);

	// Now we know that our FIN is ACK-ed, safe to close
	closesocket(socket);
}

inline void Connection::Init()
{
	debug("Initializing global connection data");
}

inline void Connection::Final()
{
}

inline Connection::Connection(const char* host, int port, bool tls) :
	m_host(host ? host : ""), m_port(port), m_tls(tls)
{
	debug("Creating Connection");

	m_readBuf.Reserve(nzbget_rs_dqh_conn_calc_read_buffer_cap(
		CONNECTION_READBUFFER_SIZE, DECODE_SLACK));
#ifndef DISABLE_TLS
	m_certVerifLevel = Options::ECertVerifLevel::cvStrict;
#endif
}

inline Connection::Connection(SOCKET socket, bool tls)
{
	debug("Creating Connection");

	m_port = 0;
	m_tls = tls;
	m_status = csConnected;
	m_socket = socket;
	m_host = GetRemoteAddr();
	m_bufAvail = 0;
	m_timeout = 60;
	m_suppressErrors = true;
	m_readBuf.Reserve(nzbget_rs_dqh_conn_calc_read_buffer_cap(
		CONNECTION_READBUFFER_SIZE, DECODE_SLACK));
#ifndef DISABLE_TLS
	m_tlsSocket = nullptr;
	m_tlsError = false;
	m_certVerifLevel = Options::ECertVerifLevel::cvStrict;
#endif
}

inline Connection::~Connection()
{
	debug("Destroying Connection");

	Connection::Disconnect();
}

inline void Connection::SetSuppressErrors(bool suppressErrors)
{
	m_suppressErrors = suppressErrors;
#ifndef DISABLE_TLS
	if (m_tlsSocket)
	{
		m_tlsSocket->SetSuppressErrors(suppressErrors);
	}
#endif
}

inline bool Connection::Connect()
{
	debug("Connecting");

	if (m_status == csConnected)
	{
		return true;
	}

	bool res = DoConnect();

	if (res)
	{
		m_status = csConnected;
	}
	else
	{
		DoDisconnect();
	}

	return res;
}

inline bool Connection::Disconnect()
{
	debug("Disconnecting");

	if (m_status == csDisconnected)
	{
		return true;
	}

	DoDisconnect();

	m_status = csDisconnected;
	m_socket = INVALID_SOCKET;
	m_bufAvail = 0;

	return true;
}

inline bool Connection::Bind()
{
	debug("Binding");

	if (m_status == csListening)
	{
		return true;
	}

	int errcode = 0;

	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		struct sockaddr_un addr;
		addr.sun_family = AF_UNIX;
		if (nzbget_rs_dqh_conn_is_unix_path_too_long(m_host.size(), sizeof(addr.sun_path)) != 0)
		{
			ReportError("Binding socket failed for %s: name too long\n", m_host.c_str(), false);
			return false;
		}
		strcpy(addr.sun_path, m_host.c_str());

		m_socket = socket(PF_UNIX, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		unlink(m_host.c_str());

		if (bind(m_socket, (struct sockaddr *)&addr, sizeof(addr)) == -1)
		{
			// Connection failed
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}
	}
	else
	{
#ifdef HAVE_GETADDRINFO
		struct addrinfo addr_hints, *addr_list, *addr;

		memset(&addr_hints, 0, sizeof(addr_hints));
		addr_hints.ai_family = nzbget_rs_ccm_conn_select_ai_family(
			(int)m_ipVersion, (int)ipV4, (int)ipV6, AF_INET, AF_INET6, AF_UNSPEC);
		addr_hints.ai_socktype = SOCK_STREAM;
		addr_hints.ai_flags = AI_PASSIVE;    // For wildcard IP address

		NzbgetRsBuf portStrBuf = nzbget_rs_dqh_conn_format_port_str(m_port);
		BString<100> portStr("%s", portStrBuf.data ? portStrBuf.data : "0");
		nzbget_rs_free(portStrBuf);

		int res = getaddrinfo(m_host.c_str(), portStr, &addr_hints, &addr_list);
		if (res != 0)
		{
			int gaiErr = nzbget_rs_dqh_conn_resolve_gai_report_err_code(res, EAI_SYSTEM);
			ReportError("Could not resolve hostname %s", m_host.c_str(), true,
				gaiErr,
				gaiErr != 0 ? gai_strerror(res) : nullptr);
			return false;
		}

		m_socket = INVALID_SOCKET;
		for (addr = addr_list; addr != nullptr; addr = addr->ai_next)
		{
			m_socket = socket(addr->ai_family, addr->ai_socktype | SOCKET_CLOEXEC, addr->ai_protocol);
#ifndef SOCK_CLOEXEC
			SetCloseOnExec(m_socket);
#endif
			if (m_socket != INVALID_SOCKET)
			{
				int opt = 1;
				setsockopt(m_socket, SOL_SOCKET, SO_REUSEADDR, (char*)&opt, sizeof(opt));
				res = bind(m_socket, addr->ai_addr, addr->ai_addrlen);
				if (res != -1)
				{
					// Connection established
					break;
				}
				// Connection failed
				errcode = GetLastNetworkError();
				closesocket(m_socket);
				m_socket = INVALID_SOCKET;
			}
		}

		freeaddrinfo(addr_list);

#else

		struct sockaddr_in	sSocketAddress;
		memset(&sSocketAddress, 0, sizeof(sSocketAddress));
		sSocketAddress.sin_family = AF_INET;
		if (m_host.empty())
		{
			sSocketAddress.sin_addr.s_addr = htonl(INADDR_ANY);
		}
		else
		{
			sSocketAddress.sin_addr.s_addr = ResolveHostAddr(m_host.c_str());
			if (sSocketAddress.sin_addr.s_addr == INADDR_NONE)
			{
				return false;
			}
		}
		sSocketAddress.sin_port = htons(m_port);

		m_socket = socket(PF_INET, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		int opt = 1;
		setsockopt(m_socket, SOL_SOCKET, SO_REUSEADDR, (char*)&opt, sizeof(opt));

		int res = bind(m_socket, (struct sockaddr *) &sSocketAddress, sizeof(sSocketAddress));
		if (res == -1)
		{
			// Connection failed
			errcode = GetLastNetworkError();
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}
#endif
	}

	if (m_socket == INVALID_SOCKET)
	{
		ReportError("Binding socket failed for %s", m_host.c_str(), true, errcode);
		return false;
	}

	if (listen(m_socket, 100) < 0)
	{
		ReportError("Listen on socket failed for %s", m_host.c_str(), true);
		closesocket(m_socket);
		m_socket = INVALID_SOCKET;
		return false;
	}

	m_status = csListening;

	return true;
}

inline int Connection::WriteLine(const char* buffer)
{
	//debug("Connection::WriteLine");

	if (m_status != csConnected)
	{
		return -1;
	}

	int res = send(m_socket, buffer, strlen(buffer), 0);
	if (res <= 0)
	{
		m_status = csBroken;
	}

	return res;
}

inline bool Connection::Send(const char* buffer, int size)
{
	debug("Sending data");

	if (m_status != csConnected)
	{
		return false;
	}

	int bytesSent = 0;
	while (bytesSent < size)
	{
		int res = send(m_socket, buffer + bytesSent, size-bytesSent, 0);
		if (res <= 0)
		{
			m_status = csBroken;
			return false;
		}
		bytesSent += res;
	}

	return true;
}

inline char* Connection::ReadLine(char* buffer, int size, int* bytesReadOut)
{
	if (m_status != csConnected)
	{
		return nullptr;
	}

	char* inpBuffer = buffer;
	size--; // for trailing '0'
	int bytesRead = 0;
	int bufAvail = m_bufAvail; // local variable is faster
	char* bufPtr = m_bufPtr; // local variable is faster
	while (size)
	{
		if (!bufAvail)
		{
			bufAvail = recv(m_socket, m_readBuf, CONNECTION_READBUFFER_SIZE, 0);
			if (bufAvail < 0)
			{
				ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
				m_status = csBroken;
				break;
			}
			else if (bufAvail == 0)
			{
				break;
			}
			else
			{
				m_totalBytesRead += bufAvail;
			}
			bufPtr = m_readBuf;
			m_readBuf[bufAvail] = '\0';
		}

		int foundNewline = 0;
		int len = nzbget_rs_rco_conn_read_line_chunk(bufPtr, bufAvail, size, &foundNewline);

		memcpy(inpBuffer, bufPtr, len);
		inpBuffer += len;
		bufPtr += len;
		bufAvail -= len;
		bytesRead += len;
		size -= len;

		if (foundNewline)
		{
			break;
		}
	}
	*inpBuffer = '\0';

	m_bufAvail = nzbget_rs_dqh_conn_clamp_buf_avail(bufAvail); // copy back to member
	m_bufPtr = bufPtr; // copy back to member

	if (bytesReadOut)
	{
		*bytesReadOut = bytesRead;
	}

	if (inpBuffer == buffer)
	{
		return nullptr;
	}

	return buffer;
}

inline std::unique_ptr<Connection> Connection::Accept()
{
	debug("Accepting connection");

	if (m_status != csListening)
	{
		return nullptr;
	}

	m_acceptBusy = false;
	SOCKET socket;
	for (;;)
	{
#if defined(__linux__) && defined(SOCK_CLOEXEC)
		socket = accept4(m_socket, nullptr, nullptr, SOCK_CLOEXEC);
#else
		socket = accept(m_socket, nullptr, nullptr);
		SetCloseOnExec(socket);
#endif
		if (socket == INVALID_SOCKET && m_status != csCancelled)
		{
			// a connection that failed before it was taken: take the next one
			// (the listener was torn down and rebound for each of these)
			int err = errno;
			int enonetVal = 0;
#ifdef ENONET
			enonetVal = ENONET;
#endif
			int acceptClass = nzbget_rs_rco_conn_classify_accept_errno(
				err, EINTR, ECONNABORTED, EPROTO, ENETDOWN,
				ENOPROTOOPT, EHOSTDOWN, EHOSTUNREACH, EOPNOTSUPP,
				ENETUNREACH, enonetVal, EMFILE, ENFILE, ENOBUFS, ENOMEM);
			if (acceptClass == 1)
			{
				continue;
			}
			m_acceptBusy = (acceptClass == 2);
		}
		break;
	}
	if (socket == INVALID_SOCKET && m_status != csCancelled)
	{
		ReportError("Could not accept connection for %s", m_host.c_str(), true);
	}
	if (socket == INVALID_SOCKET)
	{
		return nullptr;
	}

	InitSocketOpts(socket);

	// InitSocketOpts() applied the listener's timeout to the new socket:
	// keep the Connection's own idea of it (used by the poll/select waits)
	// in step instead of the constructor's default
	auto acceptedConnection = std::make_unique<Connection>(socket, m_tls);
	acceptedConnection->SetTimeout(m_timeout);
	return acceptedConnection;
}

inline int Connection::TryRecv(char* buffer, int size)
{
	//debug("Receiving data");

	memset(buffer, 0, size);

	int received = recv(m_socket, buffer, size, 0);

	if (received < 0)
	{
		ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
	}
	else
	{
		m_totalBytesRead += received;
	}

	return received;
}

inline bool Connection::Recv(char * buffer, int size)
{
	//debug("Receiving data (full buffer)");

	memset(buffer, 0, size);

	char* bufPtr = (char*)buffer;
	int NeedBytes = size;

	if (m_bufAvail > 0)
	{
		int len = nzbget_rs_ccm_conn_recv_buffered_chunk_len(size, m_bufAvail);
		memcpy(bufPtr, m_bufPtr, len);
		bufPtr += len;
		m_bufPtr += len;
		m_bufAvail -= len;
		NeedBytes -= len;
	}

	// Read from the socket until nothing remains
	while (NeedBytes > 0)
	{
		int received = recv(m_socket, bufPtr, NeedBytes, 0);
		// Did the recv succeed?
		if (received <= 0)
		{
			ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
			return false;
		}
		bufPtr += received;
		NeedBytes -= received;
		m_totalBytesRead += received;
	}
	return true;
}

inline bool Connection::DoConnect()
{
	debug("Do connecting");
	if (m_deadline && m_deadline->Stopped()) return false;

	m_socket = INVALID_SOCKET;

	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		struct sockaddr_un addr;
		addr.sun_family = AF_UNIX;
		if (nzbget_rs_dqh_conn_is_unix_path_too_long(m_host.size(), sizeof(addr.sun_path)) != 0)
		{
			ReportError("Connection to %s failed: name too long\n", m_host.c_str(), false);
			return false;
		}
		strcpy(addr.sun_path, m_host.c_str());

		m_socket = socket(PF_UNIX, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		if (!ConnectWithTimeout(&addr, sizeof(addr)))
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
			return false;
		}
	}
	else
	{
#ifdef HAVE_GETADDRINFO
		struct addrinfo addr_hints, *addr_list, *addr;

		memset(&addr_hints, 0, sizeof(addr_hints));
		addr_hints.ai_family = nzbget_rs_ccm_conn_select_ai_family(
			(int)m_ipVersion, (int)ipV4, (int)ipV6, AF_INET, AF_INET6, AF_UNSPEC);
		addr_hints.ai_socktype = SOCK_STREAM;

		NzbgetRsBuf portStrBuf = nzbget_rs_dqh_conn_format_port_str(m_port);
		BString<100> portStr("%s", portStrBuf.data ? portStrBuf.data : "0");
		nzbget_rs_free(portStrBuf);

		// Optimizer probes resolve in a bounded owned child, preserving m_host
		// for TLS SNI/certificate validation. AI_NUMERICHOST never queries DNS.
		std::vector<std::unique_ptr<addrinfo, decltype(&freeaddrinfo)>> resolvedLists;
		std::vector<addrinfo> resolvedItems;
		int res;
		if (!m_resolvedAddresses.empty())
		{
			addr_hints.ai_flags |= AI_NUMERICHOST;
			addr_list = nullptr;
			for (const auto& numeric : m_resolvedAddresses)
			{
				addrinfo* list = nullptr;
				if (getaddrinfo(numeric.c_str(), portStr, &addr_hints, &list) == 0 && list)
				{
					resolvedLists.emplace_back(list, &freeaddrinfo);
					for (addrinfo* item = list; item; item = item->ai_next) resolvedItems.push_back(*item);
				}
			}
			for (size_t i = 0; i < resolvedItems.size(); ++i)
				resolvedItems[i].ai_next = i + 1 < resolvedItems.size() ? &resolvedItems[i + 1] : nullptr;
			addr_list = resolvedItems.empty() ? nullptr : &resolvedItems.front();
			res = addr_list ? 0 : EAI_NONAME;
		}
		else if (m_deadline)
		{
			return false; // Probe DNS must never fall back to blocking resolution.
		}
		else res = getaddrinfo(m_host.c_str(), portStr, &addr_hints, &addr_list);
		debug("getaddrinfo for %s: %i", m_host.c_str(), res);

		if (res != 0)
		{
			int gaiErr = nzbget_rs_dqh_conn_resolve_gai_report_err_code(res, EAI_SYSTEM);
			ReportError("Could not resolve hostname %s", m_host.c_str(), true,
						gaiErr,
						gaiErr != 0 ? gai_strerror(res) : nullptr);
			return false;
		}

		std::vector<SockAddr> triedAddr;
		bool connected = false;

		for (addr = addr_list; addr != nullptr; addr = addr->ai_next)
		{
			if (m_deadline && m_deadline->Stopped()) break;
			// don't try the same combinations of ai_family, ai_socktype, ai_protocol multiple times
			SockAddr sa = { addr->ai_family, addr->ai_socktype, addr->ai_protocol };
			if (!m_deadline && std::find(triedAddr.begin(), triedAddr.end(), sa) != triedAddr.end())
			{
				continue;
			}
			triedAddr.push_back(sa);

			if (m_socket != INVALID_SOCKET)
			{
				closesocket(m_socket);
			}

			m_socket = socket(addr->ai_family, addr->ai_socktype | SOCKET_CLOEXEC, addr->ai_protocol);
#ifndef SOCK_CLOEXEC
			SetCloseOnExec(m_socket);
#endif
			if (m_socket == INVALID_SOCKET)
			{
				// try another addr/family/protocol
				continue;
			}

			if (ConnectWithTimeout(addr->ai_addr, addr->ai_addrlen))
			{
				// Connection established
				connected = true;
				break;
			}
		}

		if (nzbget_rs_dqh_conn_should_report_socket_create_failed(
			m_socket == INVALID_SOCKET ? 1 : 0, addr_list ? 1 : 0) != 0)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
		}

		if (nzbget_rs_dqh_conn_should_cleanup_unconnected_socket(
			connected ? 1 : 0, m_socket == INVALID_SOCKET ? 1 : 0) != 0)
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}

		if (resolvedLists.empty()) freeaddrinfo(addr_list);

		if (m_socket == INVALID_SOCKET)
		{
			return false;
		}

#else

		struct sockaddr_in	sSocketAddress;
		memset(&sSocketAddress, 0, sizeof(sSocketAddress));
		sSocketAddress.sin_family = AF_INET;
		sSocketAddress.sin_port = htons(m_port);
		sSocketAddress.sin_addr.s_addr = ResolveHostAddr(m_host.c_str());
		if (sSocketAddress.sin_addr.s_addr == INADDR_NONE)
		{
			return false;
		}

		m_socket = socket(PF_INET, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		if (!ConnectWithTimeout(&sSocketAddress, sizeof(sSocketAddress)))
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
			return false;
		}
#endif
	}

	if (!InitSocketOpts(m_socket))
	{
		return false;
	}

#ifndef DISABLE_TLS
	if (m_tls && !StartTls(true, "", ""))
	{
		return false;
	}
#endif

	return true;
}

inline bool Connection::InitSocketOpts(SOCKET socket)
{
	struct timeval TimeVal;
	TimeVal.tv_sec = m_timeout;
	TimeVal.tv_usec = 0;
	char* optbuf = (char*)&TimeVal;
	int optsize = sizeof(TimeVal);
	int err = setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, optbuf, optsize);
	if (err != 0)
	{
		ReportError("Socket initialization failed for %s", m_host.c_str(), true);
		return false;
	}
	err = setsockopt(socket, SOL_SOCKET, SO_SNDTIMEO, optbuf, optsize);
	if (err != 0)
	{
		ReportError("Socket initialization failed for %s", m_host.c_str(), true);
		return false;
	}
	return true;
}

inline bool Connection::ConnectWithTimeout(void* address, int address_len)
{
	int flags = 0, error = 0, ret = 0;
	socklen_t len = sizeof(error);

	//set socket nonblocking flag
	flags = fcntl(m_socket, F_GETFL, 0);
	if (flags < 0)
	{
		return false;
	}

	if (fcntl(m_socket, F_SETFL, flags | O_NONBLOCK) < 0)
	{
		return false;
	}

	//initiate non-blocking connect
	ret = connect(m_socket, (struct sockaddr*)address, address_len);
	if (ret < 0)
	{
		int err = GetLastNetworkError();
		if (err != EINPROGRESS)
		{
			return false;
		}
	}

	//connect succeeded right away?
	if (ret != 0)
	{
		// poll: an fd_set holds descriptors below 1024 only, and FD_SET of a
		// higher one (many connections, a raised descriptor limit) wrote past it
		struct pollfd pfd;
		pfd.fd = m_socket;
		pfd.events = POLLIN | POLLOUT;
		pfd.revents = 0;
		if (m_deadline)
		{
			ret = m_deadline->Wait(m_socket, true, true) ? 1 : 0;
			pfd.revents = POLLIN | POLLOUT;
		}
		else ret = poll(&pfd, 1, nzbget_rs_ccm_conn_poll_timeout_ms(m_timeout));
		//we are waiting for connect to complete now
		if (ret < 0)
		{
			return false;
		}
		if (ret == 0)
		{
			//we had a timeout
			errno = ETIMEDOUT;
			return false;
		}

		if (!nzbget_rs_ccm_conn_poll_revents_ready(pfd.revents, POLLIN | POLLOUT | POLLERR | POLLHUP))
		{
			return false;
		}
		//we had a positivite return so a descriptor is ready

		if (getsockopt(m_socket, SOL_SOCKET, SO_ERROR, (char*)&error, &len) < 0)
		{
			return false;
		}

		//check if we had a socket error
		if (error)
		{
			errno = error;
			return false;
		}
	}

	//put socket back in blocking mode
	if (m_deadline) return !m_deadline->Stopped();
	if (fcntl(m_socket, F_SETFL, flags) < 0)
	{
		return false;
	}

	return true;
}

inline void Connection::DoDisconnect()
{
	debug("Do disconnecting");

	if (m_socket != INVALID_SOCKET)
	{
#ifndef DISABLE_TLS
		CloseTls();
#endif
		int is_listening = 0;
		socklen_t len = sizeof(is_listening);
		bool isUnixPath = nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0;
		int sockoptRes = isUnixPath ? getsockopt(m_socket, SOL_SOCKET, SO_ACCEPTCONN, &is_listening, &len) : -1;
		if (nzbget_rs_dqh_conn_should_unlink_listening_unix_socket(
			isUnixPath ? 1 : 0, sockoptRes, is_listening) != 0)
		{
			unlink(m_host.c_str());
		}
		if (m_gracefull)
		{
			closesocket_gracefully(m_socket);
		}
		else
		{
			closesocket(m_socket);
		}
		m_socket = INVALID_SOCKET;
	}

	m_status = csDisconnected;
}

inline void Connection::ReadBuffer(char** buffer, int *bufLen)
{
	*bufLen = m_bufAvail;
	*buffer = m_bufPtr;
	m_bufAvail = 0;
}

inline void Connection::Cancel()
{
	debug("Cancelling connection");
	if (m_socket != INVALID_SOCKET)
	{
		m_status = csCancelled;

		if (m_forceClose)
		{
			SOCKET socket = m_socket;
			m_socket = INVALID_SOCKET;
			shutdown(socket, SHUT_RDWR);
			closesocket(socket);
		}
		else
		{
			shutdown(m_socket, SHUT_RDWR);
		}
	}
}

inline int Connection::GetLastNetworkError()
{
	return errno;
}

inline void Connection::ReportError(const char* msgPrefix, const char* msgArg, bool printErrCode, int errCode, const char* errMsg)
{
#ifndef DISABLE_TLS
	if (m_tlsError)
	{
		// TLS-Error was already reported
		m_tlsError = false;
		return;
	}
#endif

	BString<1024> errPrefix(msgPrefix, msgArg);

	if (printErrCode)
	{
		BString<1024> printErrMsg;
		errCode = nzbget_rs_dqh_conn_resolve_report_err_code(errCode, GetLastNetworkError());
		if (errMsg)
		{
			printErrMsg = errMsg;
		}
		else
		{
			printErrMsg = strerror(errCode);
		}

		NzbgetRsBuf formattedErr = nzbget_rs_dqh_conn_format_report_err_with_code(
			*errPrefix, errCode, (const char*)printErrMsg);
		if (m_suppressErrors)
		{
			debug("%s", formattedErr.data ? formattedErr.data : "");
		}
		else
		{
			PrintError(formattedErr.data ? formattedErr.data : "");
		}
		nzbget_rs_free(formattedErr);
	}
	else
	{
		if (m_suppressErrors)
		{
			debug("%s", *errPrefix);
		}
		else
		{
			PrintError(errPrefix);
		}
	}
}

inline void Connection::PrintError(const char* errMsg)
{
	error("%s", errMsg);
}

#ifndef DISABLE_TLS
inline bool Connection::StartTls(bool isClient, const char* certFile, const char* keyFile)
{
	debug("Starting TLS");

	m_tlsSocket = std::make_unique<TlsSocket>(m_socket, isClient, m_host.c_str(), certFile, keyFile, m_cipher.c_str(), m_certVerifLevel);
	m_tlsSocket->SetSuppressErrors(m_suppressErrors);
	m_tlsSocket->SetDeadline(m_deadline);

	bool started = m_tlsSocket->Start();
	m_tlsProbeFailure = m_tlsSocket->GetProbeFailure();
	return started;
}

inline void Connection::CloseTls()
{
	if (m_tlsSocket)
	{
		m_tlsSocket->Close();
		m_tlsSocket.reset();
	}
}

inline int Connection::recv(SOCKET s, char* buf, int len, int flags)
{
	int received = 0;

	if (m_tlsSocket)
	{
		m_tlsError = false;
		received = m_tlsSocket->Recv(buf, len);
		if (received < 0)
		{
			m_tlsError = true;
			return -1;
		}
	}
	else
	{
		received = ::recv(s, buf, len, flags);
	}
	return received;
}

inline int Connection::send(SOCKET s, const char* buf, int len, int flags)
{
	int sent = 0;

	if (m_tlsSocket)
	{
		m_tlsError = false;
		sent = m_tlsSocket->Send(buf, len);
		if (sent < 0)
		{
			m_tlsError = true;
			return -1;
		}
		return sent;
	}
	else
	{
		sent = ::send(s, buf, len, flags);
		return sent;
	}
}
#endif

#ifndef HAVE_GETADDRINFO
inline in_addr_t Connection::ResolveHostAddr(const char* host)
{
	in_addr_t uaddr = inet_addr(host);
	if (uaddr == INADDR_NONE)
	{
		struct hostent* hinfo;
		bool err = false;
		int h_errnop = 0;
#ifdef HAVE_GETHOSTBYNAME_R
		struct hostent hinfobuf;
		char strbuf[1024];
#ifdef HAVE_GETHOSTBYNAME_R_6
		err = gethostbyname_r(host, &hinfobuf, strbuf, sizeof(strbuf), &hinfo, &h_errnop);
		err = err || (hinfo == nullptr); // error on null hinfo (means 'no entry')
#endif
#ifdef HAVE_GETHOSTBYNAME_R_5
		hinfo = gethostbyname_r(host, &hinfobuf, strbuf, sizeof(strbuf), &h_errnop);
		err = hinfo == nullptr;
#endif
#ifdef HAVE_GETHOSTBYNAME_R_3
		//NOTE: gethostbyname_r with three parameters were not tested
		struct hostent_data hinfo_data;
		hinfo = gethostbyname_r((char*)host, (struct hostent*)hinfobuf, &hinfo_data);
		err = hinfo == nullptr;
#endif
#else
		std::lock_guard<std::mutex> guard(m_getHostByNameMutex);
		hinfo = gethostbyname(host);
		err = hinfo == nullptr;
#endif
		if (err)
		{
			ReportError("Could not resolve hostname %s", host, true, h_errnop, hstrerror(h_errnop));
			return INADDR_NONE;
		}

		memcpy(&uaddr, hinfo->h_addr_list[0], sizeof(uaddr));
	}
	return uaddr;
}
#endif

inline const char* Connection::GetRemoteAddr()
{
	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		return "-";
	}

	m_remoteAddr.Clear();

	char peerName[1024];
	int peerNameLength = sizeof(peerName);
	if (getpeername(m_socket, (sockaddr*)&peerName, (SOCKLEN_T*)&peerNameLength) >= 0)
	{
		inet_ntop(((sockaddr_in*)&peerName)->sin_family,
			((sockaddr_in*)&peerName)->sin_family == AF_INET6 ?
			(void*)&((sockaddr_in6*)&peerName)->sin6_addr :
			(void*)&((sockaddr_in*)&peerName)->sin_addr,
			m_remoteAddr, m_remoteAddr.Capacity());
		m_remoteAddr[m_remoteAddr.Capacity() - 1] = '\0';
	}

	return m_remoteAddr;
}

inline int Connection::FetchTotalBytesRead()
{
	int total = m_totalBytesRead;
	m_totalBytesRead = 0;
	return total;
}

class WebDownloader : public Thread, public Subject
{
public:
	enum EStatus
	{
		adUndefined,
		adRunning,
		adFinished,
		adFailed,
		adRetry,
		adNotFound,
		adRedirect,
		adConnectError,
		adFatalError
	};

	WebDownloader();
	EStatus GetStatus() { return m_status; }
	virtual void Run();
	virtual void Stop();
	EStatus Download();
	EStatus DownloadWithRedirects(int maxRedirects);
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	const char* GetInfoName() { return m_infoName; }
	void SetUrl(const char* url);
	const char* GetOutputFilename() { return m_outputFilename; }
	void SetOutputFilename(const char* outputFilename) { m_outputFilename = outputFilename; }
	time_t GetLastUpdateTime() { return m_lastUpdateTime; }
	void SetLastUpdateTimeNow();
	bool GetConfirmedLength() { return m_confirmedLength; }
	// the status code of the last response (0 before any)
	int GetHttpStatus() const { return m_httpStatus; }
	const char* GetOriginalFilename() { return m_originalFilename; }
	void SetForce(bool force) { m_force = force; }
	void SetRetry(bool retry) { m_retry = retry; }
#ifndef DISABLE_TLS
	void SetCertVerifLevel(unsigned int level) { m_certVerifLevel = level; }
#endif

	void LogDebugInfo();

protected:
	virtual void ProcessHeader(const char* line);

private:
	CString m_url;
	CString m_outputFilename;
	std::unique_ptr<Connection> m_connection;
	Mutex m_connectionMutex;
	EStatus m_status = adUndefined;
	std::atomic<time_t> m_lastUpdateTime{0};
	CString m_infoName;
	DiskFile m_outFile;
	int m_contentLen;
	bool m_confirmedLength = false;
	int m_httpStatus = 0;
	CString m_originalFilename;
	bool m_force = false;
	bool m_redirecting;
	bool m_redirected;
	bool m_gzip;
	bool m_retry = true;
#ifndef DISABLE_TLS
	unsigned int m_certVerifLevel = Options::ECertVerifLevel::cvStrict;
#endif
#ifndef DISABLE_GZIP
	std::unique_ptr<GUnzipStream> m_gUnzipStream;
#endif

	void SetStatus(EStatus status);
	bool Write(void* buffer, int len);
	bool PrepareFile();
	void FreeConnection();
	EStatus CheckResponse(const char* response);
	EStatus CreateConnection(URL *url);
	void ParseFilename(const char* contentDisposition);
	void SendHeaders(URL *url);
	EStatus DownloadHeaders();
	EStatus DownloadBody();
	void ParseRedirect(const char* location);
};

inline WebDownloader::WebDownloader()
	: m_contentLen(0)
	, m_redirecting(false)
	, m_redirected(false)
	, m_gzip(false)
{
	debug("Creating WebDownloader");

	SetLastUpdateTimeNow();
}

inline void WebDownloader::SetUrl(const char* url)
{
	m_url = WebUtil::UrlEncode(url);
}

inline void WebDownloader::SetStatus(EStatus status)
{
	m_status = status;
	Notify(nullptr);
}

inline void WebDownloader::SetLastUpdateTimeNow()
{
	m_lastUpdateTime = Util::CurrentTime();
}

inline void WebDownloader::Run()
{
	debug("Entering WebDownloader-loop");

	SetStatus(adRunning);

	int remainedDownloadRetries = 1;
	int remainedConnectRetries = 1;
	nzbget_rs_ntw_webdl_calc_initial_retries(
		g_Options->GetUrlRetries(), m_retry ? 1 : 0,
		&remainedDownloadRetries, &remainedConnectRetries);

	EStatus Status = adFailed;

	while (nzbget_rs_dqh_webdl_should_continue_retry_loop(
		IsStopped() ? 1 : 0, remainedDownloadRetries, remainedConnectRetries) != 0)
	{
		SetLastUpdateTimeNow();

		Status = DownloadWithRedirects(5);

		if (nzbget_rs_ntw_webdl_should_wait_before_retry(
			(int)Status, (int)adFailed, (int)adConnectError,
			remainedDownloadRetries, remainedConnectRetries,
			IsStopped() ? 1 : 0, m_force ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0) != 0)
		{
			detail("Waiting %i sec to retry", g_Options->GetUrlInterval());
			int msec = 0;
			while (nzbget_rs_dqh_webdl_should_keep_waiting_retry_interval(
				IsStopped() ? 1 : 0, msec, g_Options->GetUrlInterval(),
				m_force ? 1 : 0, g_WorkState->GetPauseDownload() ? 1 : 0) != 0)
			{
				// waiting isn't hanging: an interval longer than UrlTimeout got the
				// download cancelled and restarted with all its retries again
				SetLastUpdateTimeNow();
				Util::Sleep(100);
				msec += 100;
			}
		}

		if (IsStopped() || (!m_force && g_WorkState->GetPauseDownload()))
		{
			Status = adRetry;
			break;
		}

		if (nzbget_rs_dqh_webdl_is_terminal_retry_status(
			(int)Status, (int)adFinished, (int)adFatalError, (int)adNotFound) != 0)
		{
			break;
		}

		nzbget_rs_webdl_step_retry_counters(
			(int)Status, (int)adConnectError,
			remainedDownloadRetries, remainedConnectRetries,
			&remainedDownloadRetries, &remainedConnectRetries);
	}

	if (Status != adFinished && Status != adRetry)
	{
		Status = adFailed;
	}

	if (Status == adFailed)
	{
		if (IsStopped())
		{
			detail("Download %s cancelled", *m_infoName);
		}
		else
		{
			error("Download %s failed", *m_infoName);
		}
	}

	if (Status == adFinished)
	{
		detail("Download %s completed", *m_infoName);
	}

	SetStatus(Status);

	debug("Exiting WebDownloader-loop");
}

inline WebDownloader::EStatus WebDownloader::Download()
{
	EStatus Status = adRunning;

	URL url(m_url);

	Status = CreateConnection(&url);
	if (Status != adRunning)
	{
		return Status;
	}

	m_connection->SetTimeout(g_Options->GetUrlTimeout());
	m_connection->SetSuppressErrors(false);

	// connection
	bool connected = m_connection->Connect();
	if (nzbget_rs_dqh_webdl_is_connect_aborted(
		connected ? 1 : 0, IsStopped() ? 1 : 0) != 0)
	{
		FreeConnection();
		return adConnectError;
	}

	// Okay, we got a Connection. Now start downloading.
	detail("Downloading %s", *m_infoName);

	SendHeaders(&url);

	Status = DownloadHeaders();

	if (nzbget_rs_dqh_webdl_should_download_body((int)Status, (int)adRunning) != 0)
	{
		Status = DownloadBody();
	}

	if (IsStopped())
	{
		Status = adFailed;
	}

	FreeConnection();

	if (nzbget_rs_dqh_webdl_should_delete_broken_output((int)Status, (int)adFinished) != 0)
	{
		// Download failed, delete broken output file
		FileSystem::DeleteFile(m_outputFilename);
	}

	return Status;
}

inline WebDownloader::EStatus WebDownloader::DownloadWithRedirects(int maxRedirects)
{
	// do sync download, following redirects
	EStatus status = adRedirect;
	while (status == adRedirect && maxRedirects >= 0)
	{
		maxRedirects--;
		status = Download();
	}

	if (status == adRedirect)
	{
		warn("Too many redirects for %s", *m_infoName);
		status = adFailed;
	}

	return status;
}

inline WebDownloader::EStatus WebDownloader::CreateConnection(URL *url)
{
	int tlsDisabled = 0;
#ifdef DISABLE_TLS
	tlsDisabled = 1;
#endif
	int port = 0;
	int tlsInt = 0;
	int rc = nzbget_rs_rco_webdl_resolve_protocol_port(
		url->IsValid() ? 1 : 0, url->GetProtocol(), url->GetPort(),
		tlsDisabled, &port, &tlsInt);
	if (rc == -1)
	{
		error("URL is not valid: %s", url->GetAddress());
		return adFatalError;
	}
	if (rc == -2)
	{
		error("Unsupported protocol in URL: %s", url->GetAddress());
		return adFatalError;
	}
	if (rc == -3)
	{
		error("Program was compiled without TLS/SSL-support. Cannot download using https protocol. URL: %s", url->GetAddress());
		return adFatalError;
	}
	bool tls = tlsInt != 0;

	m_connection = std::make_unique<Connection>(url->GetHost(), port, tls);

#ifndef DISABLE_TLS
	m_connection->SetCertVerifLevel(m_certVerifLevel);
#endif

	return adRunning;
}

inline void WebDownloader::SendHeaders(URL *url)
{
	// retrieve file
	// not cut to a fixed size: a long (signed) url lost its "HTTP/1.0" and line end
	NzbgetRsBuf reqLineBuf = nzbget_rs_tpr_webdl_format_get_request_line(url->GetResource());
	m_connection->WriteLine(reqLineBuf.data ? reqLineBuf.data : "");
	nzbget_rs_free(reqLineBuf);

	NzbgetRsBuf uaLineBuf = nzbget_rs_tpr_webdl_format_user_agent_header(Util::VersionRevision());
	m_connection->WriteLine(uaLineBuf.data ? uaLineBuf.data : "");
	nzbget_rs_free(uaLineBuf);

	int isDefaultPort = nzbget_rs_ntw_webdl_is_default_http_port(
		url->GetProtocol(), url->GetPort());
	NzbgetRsBuf hostLineBuf = nzbget_rs_tpr_webdl_format_host_header(
		isDefaultPort, url->GetHost(), url->GetPort());
	m_connection->WriteLine(hostLineBuf.data ? hostLineBuf.data : "");
	nzbget_rs_free(hostLineBuf);

	m_connection->WriteLine("Accept: */*\r\n");
#ifndef DISABLE_GZIP
	m_connection->WriteLine("Accept-Encoding: gzip\r\n");
#endif
	m_connection->WriteLine("Connection: close\r\n");
	m_connection->WriteLine("\r\n");
}

inline WebDownloader::EStatus WebDownloader::DownloadHeaders()
{
	EStatus Status = adRunning;

	m_confirmedLength = false;
	CharBuffer lineBuf(1024*10);
	m_contentLen = -1;
	bool firstLine = true;
	m_gzip = false;
	m_redirecting = false;
	m_redirected = false;

	// Headers
	while (!IsStopped())
	{
		SetLastUpdateTimeNow();

		int len = 0;
		char* line = m_connection->ReadLine(lineBuf, lineBuf.Size(), &len);

		if (firstLine)
		{
			Status = CheckResponse(line);
			if (Status != adRunning)
			{
				break;
			}
			firstLine = false;
		}

		// Have we encountered a timeout?
		if (!line)
		{
			if (!IsStopped())
			{
				warn("URL %s failed: Unexpected end of file", *m_infoName);
			}
			Status = adFailed;
			break;
		}

		debug("Header: %s", line);

		// detect body of response
		if (nzbget_rs_ntw_webdl_is_header_end_line((unsigned char)*line) != 0)
		{
			break;
		}

		Util::TrimRight(line);
		ProcessHeader(line);

		if (m_redirected)
		{
			Status = adRedirect;
			break;
		}
	}

	return Status;
}

inline WebDownloader::EStatus WebDownloader::DownloadBody()
{
	EStatus Status = adRunning;

	m_outFile.Close();
	bool end = false;
	CharBuffer lineBuf(1024*10);
	int writtenLen = 0;

#ifndef DISABLE_GZIP
	m_gUnzipStream.reset();
	if (m_gzip)
	{
		m_gUnzipStream = std::make_unique<GUnzipStream>(1024*10);
	}
#endif

	// Body
	while (!IsStopped())
	{
		SetLastUpdateTimeNow();

		char* buffer;
		int len;
		m_connection->ReadBuffer(&buffer, &len);
		if (nzbget_rs_dqh_webdl_should_try_recv_when_buf_empty(len) != 0)
		{
			len = m_connection->TryRecv(lineBuf, lineBuf.Size());
			buffer = lineBuf;
		}

		if (len <= 0)
		{
			if (nzbget_rs_rco_webdl_check_body_chunk(
				len, m_contentLen, writtenLen, m_gzip ? 1 : 0, m_confirmedLength ? 1 : 0) == 1)
			{
				end = true;
				break;
			}
			if (!IsStopped())
			{
				warn("URL %s failed: Unexpected end of file", *m_infoName);
			}
			Status = adFailed;
			break;
		}

		// write to output file
		if (!Write(buffer, len))
		{
			Status = adFatalError;
			break;
		}
		int bodyStep = nzbget_rs_rco_webdl_check_body_chunk(
			len, m_contentLen, writtenLen, m_gzip ? 1 : 0, m_confirmedLength ? 1 : 0);
		writtenLen += len;

		if (bodyStep == 2)
		{
			end = true;
			break;
		}
	}

#ifndef DISABLE_GZIP
	m_gUnzipStream.reset();
#endif

	m_outFile.Close();

	int warnIncomplete = 0;
	Status = (EStatus)nzbget_rs_webdl_resolve_body_end_status(
		end ? 1 : 0, (int)Status, IsStopped() ? 1 : 0,
		(int)adRunning, (int)adFailed, (int)adFinished, &warnIncomplete);
	if (warnIncomplete)
	{
		warn("URL %s failed: file incomplete", *m_infoName);
	}

	return Status;
}

inline WebDownloader::EStatus WebDownloader::CheckResponse(const char* response)
{
	// rust/src/webdownload.rs decides; the warnings are logged here
	NzbgetRsHttpResponse r;
	nzbget_rs_http_check_response(response, &r);
	if (r.setStatus)
	{
		m_httpStatus = r.httpStatus;
	}
	switch (r.warn)
	{
		case 1:
			if (!IsStopped())
			{
				warn("URL %s: Connection closed by remote host", *m_infoName);
			}
			break;

		case 2:
			warn("URL %s failed: %s", *m_infoName, response);
			break;

		case 3:
			warn("URL %s failed: %s", *m_infoName, response + r.statusOffset);
			break;
	}
	switch (r.result)
	{
		case 0:
			return adRunning;

		case 1:
			m_redirecting = true;
			return adRunning;

		case 2:
			return adConnectError;

		case 3:
			return adNotFound;

		default:
			return adFailed;
	}
}

inline void WebDownloader::ProcessHeader(const char* line)
{
	// rust/src/webdownload.rs
	int value = 0;
	switch (nzbget_rs_http_header(line, m_redirecting, &value))
	{
		case 1:
			m_contentLen = value;
			m_confirmedLength = true;
			break;

		case 2:
			m_gzip = true;
			break;

		case 3:
			ParseFilename(line);
			break;

		case 4:
			ParseRedirect(line + value);
			m_redirected = true;
			break;
	}
}

inline void WebDownloader::ParseFilename(const char* contentDisposition)
{
	CString filename = WebUtil::ParseContentDispositionFilename(contentDisposition);
	if (filename.Empty())
	{
		return;
	}

	m_originalFilename = FileSystem::BaseFileName(filename);

	debug("OriginalFilename: %s", *m_originalFilename);
}

inline void WebDownloader::ParseRedirect(const char* location)
{
	// rust/src/webdownload.rs resolves the address
	NzbgetRsBuf buf = nzbget_rs_http_redirect(m_url, location);
	std::string newLocation(buf.data, buf.len);
	nzbget_rs_free(buf);
	// without the query: it carries api keys and passwords (indexer links do),
	// and this went into the log
	auto noQuery = [](const char* url) { return std::string(url, nzbget_rs_ntw_webdl_strip_query_len(url)); };
	detail("URL %s redirected to %s", noQuery(m_url).c_str(), noQuery(newLocation.c_str()).c_str());
	SetUrl(newLocation.c_str());
}

inline bool WebDownloader::Write(void* buffer, int len)
{
	if (nzbget_rs_dqh_webdl_needs_prepare_file(m_outFile.Active() ? 1 : 0) != 0 && !PrepareFile())
	{
		return false;
	}

#ifndef DISABLE_GZIP
	if (m_gzip)
	{
		m_gUnzipStream->Write(buffer, len);
		const void *outBuf;
		int outLen = 1;
		while (outLen > 0)
		{
			GUnzipStream::EStatus gZStatus = m_gUnzipStream->Read(&outBuf, &outLen);

			if (gZStatus == GUnzipStream::zlError)
			{
				error("URL %s: GUnzip failed", *m_infoName);
				return false;
			}

			if (outLen > 0 && nzbget_rs_dqh_webdl_is_gunzip_write_failed(
				outLen, m_outFile.Write(outBuf, outLen)) != 0)
			{
				return false;
			}

			if (gZStatus == GUnzipStream::zlFinished)
			{
				m_confirmedLength = true;
				return true;
			}
		}
		return true;
	}
	else
#endif

	return m_outFile.Write(buffer, len) > 0;
}

inline bool WebDownloader::PrepareFile()
{
	// prepare file for writing

	const char* filename = m_outputFilename;
	if (!m_outFile.Open(filename, DiskFile::omWrite))
	{
		error("Could not %s file %s", "create", filename);
		return false;
	}
	if (g_Options->GetWriteBuffer() > 0)
	{
		m_outFile.SetWriteBuffer(nzbget_rs_tpr_webdl_kb_to_bytes(g_Options->GetWriteBuffer()));
	}

	return true;
}

inline void WebDownloader::LogDebugInfo()
{
	info("      Web-Download: status=%i, LastUpdateTime=%s, filename=%s", m_status,
		*Util::FormatTime(m_lastUpdateTime.load()), FileSystem::BaseFileName(m_outputFilename));
}

inline void WebDownloader::Stop()
{
	debug("Trying to stop WebDownloader");
	Thread::Stop();
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
	debug("WebDownloader stopped successfully");
}

inline void WebDownloader::FreeConnection()
{
	if (m_connection)
	{
		debug("Releasing connection");
		Guard guard(m_connectionMutex);
		if (nzbget_rs_dqh_webdl_should_disconnect_on_free(
			(int)m_connection->GetStatus(), (int)Connection::csCancelled) != 0)
		{
			m_connection->Disconnect();
		}
		m_connection.reset();
	}
}

#endif
