# Fase 6 — Processos, syscalls, user mode e capacidades (desenho)

Estado: **DESENHO** (contrato a implementar pelo Claude; o Codex pode
programar contra ele já). Responde às issues #46 (entrega de INPUT ao
compositor) e #50 (arranque e consola da primeira shell). Decisões:
D-047…D-053, alinhadas com a **D-058** ("capability-first", PR #77) para a
Fase 15 não obrigar a refazer as syscalls. Qualquer mudança a este contrato
atualiza este ficheiro e `docs/INTERFACES.md` no mesmo PR.

Princípios:
- **O user space é hostil.** Cada número que atravessa a fronteira
  (registos de syscall, ponteiros, tamanhos, handles, ficheiros ELF) é
  validado pelo núcleo antes de ser usado.
- **Sem autoridade ambiente (D-055, D-058).** Um processo só consegue atuar
  sobre os objetos para os quais tem um handle, e começa só com os handles
  que quem o criou lhe deu. Não há nomes, caminhos nem dispositivos
  globais que se possam "abrir".

---

## 1. Processos e espaço de endereçamento (D-047)

- Um **processo** tem: PID, um `AddressSpace` próprio, uma **tabela de
  handles**, uma ou mais threads e `uid`/`gid` (sempre 0 = "sistema" até à
  Fase 15; só existem para a Fase 15 não mudar a estrutura). A metade do kernel (PML4 256–511) é a
  mesma em todos os processos: as entradas pré-alocadas na Fase 3 são
  copiadas, por isso o kernel nunca precisa de mapear nada por processo.
- Layout da metade de user (sem ASLR até à Fase 8):

| Intervalo | Uso |
|---|---|
| `0x0000_0000_0000_0000` – `0x0000_0000_000F_FFFF` | **nunca mapeado** (1 MiB: apanha null derefs e índices pequenos) |
| a partir de `0x0000_0000_0040_0000` | segmentos `PT_LOAD` do ELF (nos endereços pedidos, se válidos) |
| … | heap do processo (`mem_map`, Fase 6b) |
| `0x0000_7FFF_FFFF_0000` (topo) | pilha da thread principal, 64 KiB, com página de guarda por baixo |
| `0x0000_8000_0000_0000` e acima | metade do kernel: inacessível em ring 3 (bit U=0) |

- As páginas de user têm U=1, NX salvo `.text`, e **W^X** como no kernel.
- Cada thread de user tem também uma pilha de **kernel** (a da Fase 5); o
  `TSS.RSP0` é atualizado em cada troca de contexto para a pilha de kernel
  da thread que vai correr.

## 2. Entrada e saída do ring 3 (D-048)

- Entrada inicial: `iretq` com `CS = USER_CS (0x23)`, `SS = USER_DS
  (0x1B)`, `RFLAGS = IF | bit 1` (IOPL 0), RSP no topo da pilha de user,
  todos os outros registos a **zero** (não passar lixo do kernel).
- Interrupções/exceções vindas de ring 3 entram pela pilha RSP0; o stub faz
  `swapgs` se `CS & 3 != 0` (o TODO da Fase 2 em `trap.rs`).
- Uma exceção causada por user (#PF, #GP, #UD, #DE…) **mata o processo**
  (não o kernel); o relatório com endereços vai só para a série.
- CPU: **SMEP** (o kernel não executa páginas de user), **SMAP** (o kernel
  não acede a páginas de user fora de `copy_*_user`), **UMIP** (`sgdt`,
  `sidt`, `str`… dão #GP em ring 3). Ativados antes do primeiro processo.

## 3. ABI das syscalls (D-049)

- Instrução `syscall`. Número em `rax`; argumentos em `rdi, rsi, rdx, r10,
  r8, r9` (o `rcx`/`r11` são destruídos pelo CPU).
- Retorno em `rax`: `>= 0` sucesso (valor), `< 0` erro:

| Código | Nome | Significado |
|---|---|---|
| -1 | `E_INVAL` | argumento inválido (tamanho, flags, alinhamento) |
| -2 | `E_FAULT` | ponteiro de user inválido ou inacessível |
| -3 | `E_BADHANDLE` | handle inexistente ou fechado |
| -4 | `E_PERM` | o handle não tem o direito necessário |
| -5 | `E_NOMEM` | sem memória |
| -6 | `E_AGAIN` | operação não bloqueante sem dados |
| -7 | `E_BUSY` | recurso exclusivo já em uso |
| -8 | `E_NOENT` | não existe |
| -9 | `E_NOSYS` | syscall desconhecida |
| -10 | `E_TOOBIG` | tamanho acima do limite |
| -11 | `E_BADSIG` | reservado (Fase 16): imagem sem assinatura válida |

- **Todos** os argumentos são validados antes de qualquer efeito. Números
  de syscall fora da tabela → `E_NOSYS` (nunca indexar uma tabela sem
  verificar).
- **Capability-first (D-058):** toda a syscall com efeito fora do próprio
  processo recebe um handle como **primeiro argumento**. As únicas
  exceções são `exit`, `yield`, `clock_now`, `random_fill` e as operações
  sobre a própria tabela (`handle_close`, `handle_dup`, `handle_info`).
- As syscalls sensíveis chamam `audit::record` (sem efeito até à Fase 15),
  para a auditoria não exigir mexer em cada syscall depois.
- **Implementação (6c, D-063):**
  - a entrada é `syscall`. O FMASK limpa IF, DF, TF, AC, NT e IOPL: o SMAP fica fechado mesmo que o programa tenha posto AC = 1;
  - a seguir vêm `swapgs` e a troca para a pilha de kernel da thread;
  - a saída é **sempre por `iretq`**, pelo mesmo caminho das interrupções, e nunca por `sysret` (o `sysretq` com RIP não canónico faz #GP em ring 0 nos Intel);
  - RCX e R11 voltam com o RIP e o RFLAGS de user, como faria o `sysret`; os outros registos são preservados.
- **Números (ABI estável):**

| N.º | Chamada | Direito | Resultado |
|---|---|---|---|
| 0 | `exit(code: i32)` | — | não volta |
| 1 | `yield()` | — | 0 |
| 2 | `handle_close(h)` | — | 0 |
| 3 | `handle_dup(h, rights)` | `DUP`, e `rights` ⊆ direitos de `h` | novo handle |
| 4 | `handle_info(h)` | — | `(tipo << 32) \| direitos` (tipo 1 = CONSOLE) |
| 5 | `console_write(h, buf, len)` | `WRITE` | bytes aceites |
| 6 | `console_read(h, buf, len)` | `READ` | bytes lidos (bloqueia) |
| 7 | `clock_now()` | — | reservado (`E_NOSYS` até haver relógio) |
| 8 | `random_fill(buf, len)` | — | reservado (`E_NOSYS` até à Fase 8/16) |

## 4. Memória de user: `UserPtr` e `copy_*_user` (D-050)

- O kernel nunca desreferencia um ponteiro de user. Só existe
  `UserPtr<T>`/`UserSlice`, que só se leem/escrevem por
  `copy_from_user`/`copy_to_user`.
- Verificações: intervalo inteiro na metade de user, sem overflow em
  `ptr + len`, tamanho ≤ limite da syscall. O acesso é feito com SMAP
  aberto só durante a cópia (`stac` … `clac`).
- **Falhas recuperáveis**: se a cópia fizer #PF (página não mapeada ou sem
  permissão), o handler de #PF reconhece o RIP dentro da rotina de cópia
  (tabela de exceções) e a syscall devolve `E_FAULT`. Nunca panic por um
  ponteiro de user mau.
- **Sem TOCTOU**: os dados de user são copiados **uma vez** para o kernel e
  só depois validados e usados.

## 5. Handles e direitos (D-051, refina D-018)

- Um handle é um `u32` opaco para o processo: índice na tabela + **geração**
  (um handle fechado e reutilizado não aponta para o objeto novo).
- Direitos (`u32`, bitflags):
  - bits 0–15 **genéricos**: `READ`, `WRITE`, `MAP`, `TRANSFER`, `DUP`,
    `EXEC`; os restantes reservados;
  - bits 16–31 **específicos de cada tipo** de objeto (por exemplo
    `NET_CONNECT` na Fase 17, `AUDIT_READ` na Fase 15). Um tipo novo nunca
    muda a ABI.
- Syscalls:
  - `handle_close(h)`;
  - `handle_dup(h, rights) -> h2`: só com `DUP`, e `rights` ⊆ direitos de
    `h`. Os direitos **só se podem reduzir**;
  - `handle_info(h) -> (tipo, direitos)`: um processo pode ver o que tem
    sem tentar e falhar.
- **Criar processos:** `process_spawn(imagem, handles, n) -> h_proc` é a
  única forma de criar um processo e de lhe dar autoridade. `imagem` é um
  handle de ficheiro com `EXEC`; `handles` aponta para `n` pares
  triplos `(handle, direitos, flags)` (no máximo 64). O processo novo não
  herda nada por omissão. (Implementado na 6d, junto com o carregador ELF.)
  - **Regras (issue #79, D-063):**
    - `SPAWN_COPY` exige `DUP` e o pai fica com o handle;
    - `SPAWN_MOVE` exige `TRANSFER` e o handle **sai** do pai;
    - em ambos os casos, os direitos do filho ⊆ os direitos de origem.
  - **Tudo ou nada:** todas as entradas são validadas antes de mexer em qualquer tabela. Os handles movidos só saem do pai no **último passo**, sob o lock da tabela, quando já nada pode falhar. Se o spawn falhar, o pai mantém tudo.
  - **Erros:** um handle repetido numa lista que o move dá `E_INVAL`. Uma geração que mudou entre a validação e o último passo dá `E_BADHANDLE`, e o filho é destruído.
  - Os objetos exclusivos (INPUT e DISPLAY) não têm `DUP`, por isso só passam por `SPAWN_MOVE`.
- **Transferência por canal (Fase 12):** exige `TRANSFER`, retira o handle
  ao emissor e os direitos só podem diminuir.
- Não há nomes globais.

## 6. Consola da primeira shell (issue #50, D-052)

- **Arranque:** depois de `devices::init`, o kernel abre `/sbin/init` no
  initrd (VFS do Codex, #24/#41) e carrega-o como ELF64. Se não existir ou
  for inválido: `[init] /sbin/init missing or invalid (<razão>)` e o kernel
  continua (idle). Não há caminho alternativo nem linha de comandos.
- **Validação do ELF** (input não confiável): `ELF64`, little-endian,
  `ET_EXEC`, `EM_X86_64`; cabeçalhos dentro do ficheiro; até 16 `PT_LOAD`;
  cada segmento: `filesz ≤ memsz`, alinhamento a 4 KiB, endereços dentro da
  metade de user e ≥ 1 MiB, sem sobreposições, **nunca W+X**; entrada dentro
  de um segmento executável. Os bytes são **copiados** para frames novos (o
  initrd é só de leitura e partilhado).
- **Direitos iniciais:** o `init` (PID 1) é o único processo criado pelo
  kernel e não por `process_spawn`. Recebe:
  - o handle `0`, para o objeto **CONSOLE**, com `READ | WRITE | DUP |
    TRANSFER` (para o poder passar à shell);
  - quando o VFS estiver ligado (Fase 7), um handle **DIR** com `READ` para a
    raiz do initrd, e é com ele que o `init` abre os programas
    (`open_at(dir, "bin/sh", READ | EXEC)`);
  - quando existirem (Fases 10–11), os handles únicos de **INPUT** e
    **DISPLAY**, que o `init` passa ao compositor com `process_spawn`.

  Mais nada. A ordem fixa (0 = CONSOLE, 1 = DIR, 2 = INPUT, 3 = DISPLAY)
  faz parte da ABI do `init`.
- `console_write(h, buf, len) -> n`: escreve em todas as consolas do kernel
  (ecrã + série). `len ≤ 4096` (senão `E_TOOBIG`). Os bytes são UTF-8: os
  inválidos aparecem como `?`, e **todos os caracteres de controlo exceto
  `\n` e `\t` também** (C0, DEL e C1, incluindo ESC). Assim, um programa não
  manda sequências de escape para o terminal nem falsifica linhas do
  kernel com `\r`; **escrita parcial permitida** (devolve quantos
  bytes foram aceites); `E_FAULT` se o buffer for inválido, `E_PERM` sem
  `WRITE`.
- `console_read(h, buf, len) -> n`: **bloqueia** (`WaitQueue`) até haver
  pelo menos 1 byte; devolve até `len` bytes (sem disciplina de linha: a
  edição é da shell, #49). Codificação do fluxo de entrada:
  - texto: UTF-8 já com o layout aplicado;
  - `Enter` = `\n`, `Backspace` = `0x7F`, `Tab` = `\t`, `Ctrl+letra` =
    `0x01`–`0x1A`;
  - setas = `ESC [ A/B/C/D`, `Home`/`End` = `ESC [ H/F`, `Delete` = `ESC [ 3 ~`.
- **Origem do texto:** o driver de teclado (Codex) traduz `KeyEvent` com o
  layout e chama `console::input_push(&[u8])` (seguro em IRQ, sem heap,
  buffer circular de 4 KiB). Se o buffer encher, os bytes novos são
  **descartados** e um contador de diagnóstico regista a perda; o
  `console_read` continua a devolver os bytes guardados, nunca um erro por
  isso. Quando houver compositor (Fase 13), o teclado vai para o compositor
  e não para a consola.

## 7. Eventos de INPUT para o compositor (issue #46, D-053)

- **Formato** (`#[repr(C)]`, 24 bytes, little-endian, igual no kernel e no
  user space):

```rust
#[repr(C)]
pub struct InputEvent {
    pub time_ns: u64, // relógio monotónico, ns desde o arranque
    pub kind: u16,    // 1 = tecla, 2 = rato relativo, 3 = perdidos
    pub code: u16,    // tecla: KeyCode (posição física, #14); rato: 0
    pub value: u32,   // tecla: 0 solta, 1 premida, 2 repetição;
                      // rato: botões (bit0 esq, bit1 dir, bit2 meio)
    pub dx: i32,      // rato: deslocamento X (−256..=255); tecla: 0
    pub dy: i32,      // rato: Y (positivo = para cima); tecla: 0
}
```
- **Relógio:** `time_ns` é monotónico, em nanossegundos desde o arranque,
  do mesmo relógio que o `time::now_ns()` do kernel (TSC calibrado no
  arranque; enquanto não houver calibração, contagem de ticks × período).
- **Direito de leitura:** objeto **INPUT**, único no sistema, criado no
  arranque. **Não há `input_open()` (D-058):** o kernel dá o único handle
  ao `init`, que o passa ao compositor com `process_spawn` (Fase 13); até
  lá, nenhum outro processo o tem. A exclusividade vem de existir um só
  handle, **sem** o direito `DUP`: não se pode copiar, só transferir.
- **Leitura:** `input_read(h, buf, max) -> n` bloqueia até haver ≥ 1 evento
  e copia até `max` eventos inteiros (`buf` com `max * 24` bytes válidos).
- **Perda:** fila circular de 256 eventos por objeto INPUT. Se encher, o
  evento novo é descartado e o próximo `input_read` começa por um evento
  `kind = 3` (perdidos); o compositor deve repor o estado dos botões e
  teclas (assumir tudo solto).
- **Troca de consumidor:** ao fechar ou transferir o handle, a fila é
  **esvaziada**. O
  consumidor seguinte nunca vê teclas destinadas ao anterior (por exemplo,
  uma password).
- **Do handler de IRQ:** o driver publica com `input::push(event)` (fila
  fixa, sem heap, segura em IRQ) e **depois** acorda o leitor com
  `WaitQueue::wake_one` (sem heap desde o #39/#40). Nada aloca no caminho
  da IRQ.

---

## Ordem de implementação (Claude)

1. **6a** SMEP/SMAP/UMIP + `UserPtr` + `copy_*_user` com recuperação de #PF.
2. **6b** processos, espaços de endereçamento de user, `TSS.RSP0`, `swapgs`,
   entrada em ring 3, exceções de user matam o processo.
3. **6c** (FEITO, D-063) `syscall`/`sysret`, tabela de syscalls, handles e direitos
   (com `handle_info` e o formato de direitos da D-058), objeto CONSOLE,
   pontos de auditoria sem efeito.
4. **6d** carregador ELF, `process_spawn` e arranque de `/sbin/init` a
   partir do initrd.
5. **6e** objeto INPUT e `input_read` (com a Fase 11 do Codex).

O Codex pode, em paralelo: construir `/sbin/init` (shell) como ELF64
estático contra esta ABI, a biblioteca de syscalls em `userland/`, e o
`console::input_push` no driver de teclado.
