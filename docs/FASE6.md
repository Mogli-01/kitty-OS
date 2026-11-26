# Fase 6 — Processos, syscalls, user mode e capacidades (desenho)

Estado: **DESENHO** (contrato a implementar pelo Claude; o Codex pode
programar contra ele já). Responde às issues #46 (entrega de INPUT ao
compositor) e #50 (arranque e consola da primeira shell). Decisões:
D-047…D-053. Qualquer mudança a este contrato atualiza este ficheiro e
`docs/INTERFACES.md` no mesmo PR.

Princípio: **o user space é hostil**. Cada número que atravessa a fronteira
(registos de syscall, ponteiros, tamanhos, handles, ficheiros ELF) é
validado pelo núcleo antes de ser usado.

---

## 1. Processos e espaço de endereçamento (D-047)

- Um **processo** tem: PID, um `AddressSpace` próprio, uma **tabela de
  handles** e uma ou mais threads. A metade do kernel (PML4 256–511) é a
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

- **Todos** os argumentos são validados antes de qualquer efeito. Números
  de syscall fora da tabela → `E_NOSYS` (nunca indexar uma tabela sem
  verificar).
- `sysret` só com RIP canónico na metade de user (o `sysretq` com RIP não
  canónico faz #GP em ring 0 nalguns CPUs: verificar e, se for o caso,
  sair por `iretq`).

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
- Direitos (bitflags): `READ`, `WRITE`, `MAP`, `TRANSFER`, `DUP`.
- Syscalls: `handle_close(h)`, `handle_dup(h, rights) -> h2` (só com
  `DUP`, e `rights` ⊆ direitos de `h`: **só se pode reduzir**).
- Um processo começa só com os handles que quem o cria lhe dá. Não há nomes
  globais.

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
- **Direitos iniciais:** o `init` (PID 1) recebe **um** handle, `0`, para o
  objeto **CONSOLE** com `READ | WRITE`. Mais nada.
- `console_write(h, buf, len) -> n`: escreve em todas as consolas do kernel
  (ecrã + série). `len ≤ 4096` (senão `E_TOOBIG`); bytes UTF-8, os
  inválidos aparecem como `?`; **escrita parcial permitida** (devolve quantos
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
- **Direito de leitura:** objeto **INPUT**, único no sistema. Só um
  processo o pode ter aberto de cada vez (exclusivo): `input_open() -> h`
  devolve `E_PERM` sem a capacidade e `E_BUSY` se já houver consumidor. A
  capacidade é dada pelo `init` ao compositor (Fase 13); até lá nenhum
  processo a tem.
- **Leitura:** `input_read(h, buf, max) -> n` bloqueia até haver ≥ 1 evento
  e copia até `max` eventos inteiros (`buf` com `max * 24` bytes válidos).
- **Perda:** fila circular de 256 eventos por objeto INPUT. Se encher, o
  evento novo é descartado e o próximo `input_read` começa por um evento
  `kind = 3` (perdidos); o compositor deve repor o estado dos botões e
  teclas (assumir tudo solto).
- **Troca de consumidor:** ao fechar o handle, a fila é **esvaziada**. O
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
3. **6c** `syscall`/`sysret`, tabela de syscalls, handles e direitos,
   objeto CONSOLE.
4. **6d** carregador ELF e arranque de `/sbin/init` a partir do initrd.
5. **6e** objeto INPUT e `input_read` (com a Fase 11 do Codex).

O Codex pode, em paralelo: construir `/sbin/init` (shell) como ELF64
estático contra esta ABI, a biblioteca de syscalls em `userland/`, e o
`console::input_push` no driver de teclado.
