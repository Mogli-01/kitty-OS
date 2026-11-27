# Vetores oficiais de criptografia

O job valida os ficheiros em `vectors/` em cada PR. Cada ficheiro contém
uma referência oficial, a secção do exemplo e os bytes em hexadecimal
minúsculo. Os valores são dados públicos para ensaios.

- ChaCha20-Poly1305: RFC 8439, §2.8.2. Comparar cifragem e tag, e confirmar
  que a abertura reproduz o texto original.
- Ed25519: RFC 8032, §7.1, TEST 1. Verificar a assinatura da mensagem vazia.
- SHA-256: exemplo NIST «One Block Message Sample», mensagem `abc`.
- Argon2id: RFC 9106, §5.3. O exemplo usa segredo e dados associados.
  O adaptador deve usar a entrada de teste que aceita esses campos e
  comparar a tag. Os 32 KiB do vetor são um parâmetro do ensaio.

O job passa a chamar `cargo test --manifest-path tests/crypto/host/Cargo.toml
--locked --test official_vectors` assim que existir `kernel/src/crypto/`
ou `kernel/src/crypto.rs`. O PR da primeira implementação deve fornecer
esse adaptador com um lockfile próprio e a implementação de produção
ligada aos casos. Se o módulo chegar sem adaptador, a CI falha com uma
mensagem explícita. Até lá, a CI verifica os dados e a compilação normal
continua a cargo do workflow do kernel.

A implementação é **educativa, não auditada**, conforme D-056. Os vetores
verificam resultados funcionais e a revisão de tempo constante é separada.
