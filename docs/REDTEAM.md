# Red team vs blue team

Processo contínuo a partir da Fase 6 (D-059). Cada IA ataca a área da
**outra** e corrige a sua. O objetivo é encontrar falhas antes de um
atacante real, e aprender com cada uma.

## Âmbito e ética (obrigatório)
- Os ataques são **só** contra o kitty-OS, dentro do **QEMU** ou do hardware
  dos donos do projeto (o Rodrigo e o amigo). **Nunca** contra sistemas de
  terceiros: nada de scans, tráfego ou exploits fora da máquina virtual.
- Os testes de rede (Fase 17) usam apenas a rede virtual do QEMU (`user` ou
  `tap` isolado), sem saída para a Internet durante os ataques.
- Os PoCs não podem conter segredos reais, nem código copiado de exploits de
  terceiros com licença incompatível.
- O kitty-OS ainda não tem utilizadores reais, por isso as falhas são
  publicadas logo no repositório (que é público). Se um dia tiver, este
  ponto passa a prever um período de embargo.

## Rondas
- Há **uma ronda no fim de cada fase concluída**, a partir da Fase 6
  (6, 7, 8, …). A ronda abre quando a última tarefa da fase passa a FEITO e
  fecha quando todas as falhas críticas e altas dessa ronda estão corrigidas.
- **Red team:**
  - **o Claude ataca** a área do Codex: drivers, sistema de ficheiros, rede,
    GUI (compositor, toolkit, aplicações), shell e userland;
  - **o Codex ataca** a área do Claude: kernel, memória, syscalls, scheduler,
    capacidades, auditoria, firewall e crypto.
- **Blue team:** o dono da área corrige as falhas na sua própria área.

## Ciclo de vida de uma falha
1. **Prova.** O atacante escreve um PoC que corre **só no QEMU**, em
   `tests/security/redteam/<ronda>-<nome>/`. O PoC é um teste que verifica
   que **o ataque é bloqueado**, por isso falha enquanto a falha existir.
   - A pasta `tests/security/redteam/` é **partilhada**: é a única exceção à
     regra das áreas, e só para acrescentar PoCs das falhas que o próprio
     encontrou. O runner e a integração na CI são do Codex.
   - Para a CI não ficar vermelha por uma falha conhecida, cada PoC está
     registado num manifesto com o estado `aberta` ou `corrigida`. Com
     `aberta`, a CI espera que o teste **falhe** (e avisa se passar sem
     correção). Com `corrigida`, a CI exige que passe: passa a ser um teste
     de regressão permanente.
2. **Issue.** Uma por falha, com as etiquetas `vulnerabilidade`, `red-team`
   e `para-claude` ou `para-codex` (o dono da área), contendo:
   - **severidade:** crítica / alta / média / baixa (ver tabela abaixo);
   - **impacto:** o que um atacante consegue fazer;
   - **passos para reproduzir** e o caminho do PoC;
   - a categoria (lista abaixo).
3. **Correção (blue team).** O PR de correção:
   - faz o PoC passar (o ataque é bloqueado) e muda o estado para `corrigida`;
   - acrescenta um teste de regressão próprio, se o PoC não cobrir as
     variantes óbvias;
   - atualiza o `docs/SECURITY.md` se uma proteção nova entrar ou mudar;
   - fecha a issue (`Fixes #N`).
4. **Placar.** Quem fecha a ronda atualiza a tabela no fim deste ficheiro.

## Severidade
| Severidade | Exemplos |
|---|---|
| **crítica** | user → kernel (execução de código em ring 0); leitura ou escrita arbitrária de memória do kernel; contornar a verificação de assinaturas; recuperar a chave do disco encriptado |
| **alta** | utilizador → admin; fuga de capacidades (usar um objeto sem o handle); ler dados, ecrã ou input de outro processo/janela; contornar o firewall |
| **média** | fuga de endereços do kernel (anula o KASLR); DoS do sistema inteiro a partir de um processo sem privilégios; canal lateral de timing explorável na crypto |
| **baixa** | DoS só do próprio processo/serviço; fuga de informação sem impacto direto; falha que exige já ter privilégios de admin |

## Categorias a testar
- **Corrupção de memória:** overflows, use-after-free, double free, inteiros
  que dão a volta, parsers de dados externos (ELF, initrd, FS, pacotes de
  rede, fontes, imagens).
- **Escalada de privilégios:** user → kernel; utilizador → admin.
- **Fuga de capacidades:** usar um objeto sem ter o handle, aumentar
  direitos com `dup`/transferência, handles reciclados (geração), herdar
  handles sem os pedir.
- **Fuga de informação:** endereços do kernel, dados de outros processos,
  frames não limpos, píxeis ou input de outras janelas, registos de CPU
  não limpos na entrada em ring 3.
- **DoS:** esgotar memória do kernel, filas de IPC, tabelas de handles,
  CPU (com prioridades), floods de rede, o registo de auditoria.
- **Contorno do firewall** e **da verificação de assinaturas** (ELF,
  pacotes `kpkg`, modo de desenvolvimento ativo por engano).
- **Ataques à crypto:** timing em comparações e operações com segredos,
  nonces repetidos no ChaCha20-Poly1305, assinaturas maleáveis no Ed25519,
  parâmetros fracos no Argon2id, CSPRNG previsível ou sem entropia.

## Desafios CTF (mais tarde)
Falhas antigas e **já corrigidas** podem virar desafios em `ctf/<nome>/`,
escritos por quem encontrou a falha. Cada desafio tem:
- `README.md` com o enunciado (o que se sabe e o objetivo, sem a solução);
- a versão vulnerável (um commit fixado ou um patch sobre o `main`) e as
  instruções para a correr no QEMU;
- uma flag (`kitty{...}`) que só se consegue ler explorando a falha dentro
  da VM. As flags não são segredos: o repositório é público.
- `SOLUCAO.md` com a explicação, para quem quiser aprender depois.

## Placar
Atualizado no fim de cada ronda. "Encontradas" = falhas que a equipa
encontrou na área da outra; "corrigidas" = falhas da própria área que
corrigiu.

| Ronda (fase) | Claude: encontradas | Claude: corrigidas | Codex: encontradas | Codex: corrigidas | Notas |
|---|---|---|---|---|---|
| 1 (Fase 6) | 0 | 0 | 0 | 0 | por começar |
