#import "../oculibre.typ": *

#show: oculibre-doc.with(
  title: [Secure channel: Noise or TLS],
  subtitle: [Choosing the encryption of Wi-Fi connections (issue \#5)],
  revision: "1.0",
  date: datetime(year: 2026, month: 11, day: 11),
)

#remark(title: [Question])[Over Wi-Fi, nobody on the same network may watch the stream or inject pictures
into the user's virtual camera (threat model: protocol §13). Which secure channel goes below the
Oculibre preamble: a Noise handshake or TLS 1.3 with pinned keys? Status: *decided (2026-11-11): TLS
only, TLS 1.2 minimum, version negotiated by TLS itself*.]

= Summary <summary>

- *Decision: TLS with pinned self-signed keys and mutual authentication*: TLS 1.3 when the phone
  has it (Android 10, API 29+), TLS 1.2 restricted to ECDHE + AEAD cipher suites below. On Android it is
  the platform's TLS (Conscrypt / BoringSSL) with keys held by the Android Keystore; on the PC, `rustls`.
  *We write no cryptographic code*, only configuration, which tests can check.
- *Noise would be the better-shaped tool* (made for pinned raw keys, small, no certificates), but its
  standard key exchange is X25519 (or X448), and Android only offers X25519 from *API 33* (Android 13).
  On our minimum API 21, Noise means writing X25519 ourselves (the rule is: no runtime dependency in
  the app), the kind of code a small project should not own.
- USB is not affected: it stays plaintext over loopback (`adb reverse`), on every phone.
- Main costs of TLS: two TLS versions to test, certificate plumbing (pinning instead of a certificate
  authority) and, on the PC, sharing one TLS connection between the reading and the writing threads.

= What the channel must do <needs>

+ *Confidentiality and integrity* of every message after the preamble, with forward secrecy (a key
  stolen later must not decrypt recordings of past sessions).
+ *Mutual authentication with pinned keys*: the phone only talks to the computer it was paired with,
  and the computer only accepts paired phones. No certificate authority, no Internet.
+ *Pairing with a QR code* shown by the computer and scanned by the phone (protocol §13): the QR carries
  the computer's public key (or its hash), its addresses and a one-time token.
+ *Android API 21 and no runtime dependency* (project rule): platform APIs only.
+ *Low cost*: a 1080p60 stream is about 15 Mbit/s; the encryption must not add visible latency or heat,
  even on an old ARMv7 phone without AES instructions.
+ *Small audit surface*: the host parser is fuzzed; the security code must be reviewable by one person.

= The two candidates <candidates>

== Noise <noise>

The *Noise Protocol Framework* (Trevor Perrin, 2016; revision 34 in 2018) is not a protocol but a
recipe for building handshakes out of three primitives: a Diffie-Hellman function, an AEAD cipher and
a hash. A protocol name spells out the choices, for instance `Noise_XX_25519_ChaChaPoly_BLAKE2s`:

- `XX`, `IK`, `KK`… is the *handshake pattern*. The first letter is the initiator's static key, the
  second the responder's: `N` none, `K` already known to the other side, `X` transmitted during the
  handshake, `I` transmitted immediately (in the first message).
- `25519` or `448` (X25519, X448) is the key exchange; `ChaChaPoly` or `AESGCM` the cipher; `SHA256`,
  `SHA512`, `BLAKE2s` or `BLAKE2b` the hash. The specification defines no other key exchange.

Noise is used by WireGuard (`Noise_IKpsk2`), WhatsApp, the Lightning Network and libp2p. For Oculibre:
first pairing with `IKpsk2` (the phone knows the computer's key from the QR, the one-time token is the
pre-shared key), then `IK` or `KK` at each connection: one round trip, identities encrypted.

== TLS <tls>

TLS 1.3 (RFC 8446) is the standard secure channel; TLS 1.2 (RFC 5246) its predecessor, still secure
when limited to ephemeral ECDHE key exchange and AEAD ciphers. Without a certificate authority, each
side uses a *self-signed certificate* and the other side checks a *pinned hash of its public key*
instead of a chain of trust. Pairing: the QR carries the hash of the computer's key and a one-time
token; the phone sends the token inside the encrypted channel, and the computer then remembers the
phone's key.

= What Android offers <android>

From the Android reference pages (supported API level of each algorithm or protocol):

#table(
  columns: (1fr, auto, auto),
  table.header([Primitive], [Android name], [API level]),
  [X25519 key exchange (Noise `25519`)], [`KeyAgreement` / `KeyPairGenerator` `XDH`], [*33+*],
  [ECDH on P-256], [`KeyAgreement` `ECDH`], [11+],
  [AES-GCM (Noise `AESGCM`)], [`Cipher` `AES/GCM/NoPadding`], [10+],
  [ChaCha20-Poly1305 (Noise `ChaChaPoly`)], [`Cipher` `ChaCha20/Poly1305/NoPadding`], [28+],
  [BLAKE2s, BLAKE2b], [—], [none],
  [TLS 1.2 (client socket)], [`SSLSocket` `TLSv1.2`], [16+, enabled by default 20+],
  [TLS 1.3], [`SSLSocket` / `SSLContext` `TLSv1.3`], [*29+*],
  [#tag("TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256")], [TLS 1.2 cipher suite], [20+],
  [#tag("TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256")], [TLS 1.2 cipher suite], [24+],
)

So on API 21–32:
- *Noise*: AES-GCM and SHA-256 are there (`Noise_*_25519_AESGCM_SHA256` is a valid choice), but *not
  X25519*. The options are to implement X25519 in Kotlin (a few hundred lines, ported from a public
  reference such as TweetNaCl, checked against the RFC 7748 vectors), or to use P-256 from the platform,
  which is *not* a standard Noise function (no interoperable library, no published test vectors).
- *TLS*: everything is there. TLS 1.3 only from API 29 (Android 10); below, TLS 1.2 with
  `ECDHE_ECDSA_AES_128_GCM` (API 20+), forward secrecy included.

= Comparison <comparison>

#table(
  columns: (auto, 1fr, 1fr),
  table.header([], [Noise (`IKpsk2` / `IK`, 25519, AES-GCM, SHA-256)], [TLS (1.3, 1.2 floor) with pinned keys]),
  [Crypto code we write],
  [X25519 in Kotlin below API 33, plus the Noise state machine (about 400 lines) on Android],
  [None: platform TLS on Android, `rustls` on the PC; we write the key pinning and the pairing],
  [Old phones (API 21–28)], [Same protocol everywhere (with our X25519)], [TLS 1.2: still sound with ECDHE + AEAD, but two versions to test],
  [Key storage on Android], [Raw private key in app-private storage (the Keystore cannot hold X25519 keys before recent versions)], [Private key generated *inside the Android Keystore*, never readable by the app; the Keystore also produces the self-signed certificate],
  [Identities on the network], [Encrypted (`IK`)], [Encrypted in TLS 1.3; *visible in TLS 1.2* (certificates in clear): an observer can recognise the phone's key across sessions],
  [Message size], [Noise messages are limited to 65 535 bytes: video frames (up to about 1 MB for a 4K keyframe) must be split], [Transparent: TLS cuts the byte stream into records itself],
  [Threads (PC)], [Two independent cipher states after the handshake: one for the reader, one for the writer], [One `rustls` connection for both directions: a lock or a dedicated I/O thread (Android's `SSLSocket` already allows one reader and one writer)],
  [Datagram transports later (protocol "reserved")], [Fits naturally (explicit nonces)], [Needs DTLS or QUIC, not offered by Android's platform],
  [Libraries on the PC], [`snow`], [`rustls` (memory-safe, widely used and fuzzed), `rcgen` for the certificate],
  [Typical mistake], [A bug in our X25519 or state machine, possibly timing leaks in Kotlin], [A trust manager that accepts any certificate (the classic Android TLS bug): to be caught by tests that connect with a wrong key],
  [Security updates], [Our code: we patch it], [Platform TLS: updated with the phone; *frozen on unmaintained Android 5–8 phones*],
)

On cost: AES-GCM at 15 Mbit/s is a few percent of one core even without AES instructions (BoringSSL
uses NEON), and both candidates add one round trip at connection time: no measurable difference.

On frozen platforms: the phone is the TLS *client* and only connects to the computer whose key it
pinned, so an attacker must first take the computer's address on the network to reach the phone's TLS
code. The same applies to a hand-written Noise. Neither choice protects a phone that is compromised
itself.

= Decision <decision>

*TLS with pinned self-signed keys, mutual authentication, TLS 1.3 when available, TLS 1.2 limited to
ECDHE + AEAD suites.* The deciding argument is the project rule (no runtime dependency on Android)
combined with API 21: with Noise we would own an elliptic-curve implementation; with TLS, all the
cryptography is maintained by Google (Android) and the `rustls` project (PC), and the private key of the
phone never leaves the Keystore. Noise remains the better design on paper; it becomes the natural
choice if the minimum API one day reaches 33, or for a future datagram transport.

The protocol does not change above the channel (§3 "Layers"): preamble and messages are the same,
inside TLS.

*Version selection is TLS's own job*: the phone offers the versions it knows, the computer picks the
highest common one (TLS 1.3 on Android 10+, TLS 1.2 below), and TLS protects that choice against an
attacker forcing the older version. We only set the floor (1.2) and the allowed suites.

== Why not Noise on Android 13+ and TLS below <why-not-both>

A tempting middle way: Noise where Android has X25519 (API 33+), TLS elsewhere. Rejected:
- *No speed gain for the video*: after the handshake both encrypt the stream with the same AEAD ciphers
  (AES-GCM or ChaCha20-Poly1305) at the same speed; they only differ by the handshake, done once per
  connection.
- *Two secure channels to write, test and audit*, and the security of the whole is that of the weaker
  one.
- *A home-made selector* (Noise or TLS?) would have to resist downgrade attacks; TLS does this for its
  own versions, nobody would do it for us between Noise and TLS.
- What Noise adds is either already there (identity hiding: TLS 1.3, Android 10+) or not needed
  (datagram transports).

The door stays open at no cost: the channel is a separate layer below the preamble, and the pairing URI
carries a version (`v=1`). A later protocol version may add another channel if a real need appears.

= Plan <plan>

+ *Spike on the oldest phone available* (API 21–23 if possible) and on the S23: mutual TLS between the
  app (`SSLSocket`, Keystore EC P-256 key, custom `X509TrustManager` pinning the computer's key hash)
  and a `rustls` test server. To check: TLS 1.2 on API 21 with a Keystore-held key for client
  authentication, TLS 1.3 on API 29+, CPU cost at 1080p60.
+ Pairing: the computer shows a QR code with
  ```
  oculibre://pair?v=1&k=<SHA-256 of its public key>&t=<one-time token>&a=<addresses>
  ```
  The phone sends the token inside the channel (new message, or a `HELLO` field during pairing); the
  computer compares it in constant time, stores the phone's key hash, and refuses unpaired phones with
  `BYE unpaired`.
+ PC side: `rustls` server with a custom client-certificate verifier (pinned keys only), certificate
  generated once with `rcgen`; reader/writer split of the connection.
+ Tests: connection refused with a wrong computer key, an unpaired phone, a replayed or wrong token,
  TLS 1.1 or a non-AEAD suite; the existing session tests run over TLS.
+ Spec: protocol 0.3 (§13 Security, pairing URI, `BYE unpaired`), threat model in `SECURITY.md`.

= Sources <sources>

- Noise Protocol Framework, revision 34: #link("https://noiseprotocol.org/noise.html")
- Android reference, supported algorithms and protocols:
  #link("https://developer.android.com/reference/javax/crypto/KeyAgreement")[`KeyAgreement`],
  #link("https://developer.android.com/reference/javax/crypto/Cipher")[`Cipher`],
  #link("https://developer.android.com/reference/java/security/KeyPairGenerator")[`KeyPairGenerator`],
  #link("https://developer.android.com/reference/javax/net/ssl/SSLSocket")[`SSLSocket`],
  #link("https://developer.android.com/reference/javax/net/ssl/SSLContext")[`SSLContext`]
- RFC 8446 (TLS 1.3), RFC 5246 (TLS 1.2), RFC 7748 (X25519)
- `rustls`: #link("https://github.com/rustls/rustls"); `snow`: #link("https://github.com/mcginty/snow")
- WireGuard (Noise `IKpsk2`): #link("https://www.wireguard.com/protocol/")
