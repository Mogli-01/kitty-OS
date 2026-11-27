.globl _start
.text
_start:
  # The root handle cannot grant write authority for the read-only archive.
  mov $4, %eax
  mov $1, %edi
  lea file_path(%rip), %rsi
  mov $file_path_len, %edx
  mov $3, %r10d
  syscall
  cmp $-4, %rax
  jne failure
  mov $3, %eax
  xor %edi, %edi
  lea message(%rip), %rsi
  mov $message_len, %edx
  syscall
  xor %edi, %edi
  xor %eax, %eax
  syscall
failure:
  ud2
file_path:
  .ascii "etc/kitty.txt"
  .set file_path_len, . - file_path
message:
  .ascii "[init-test] static ELF executed from initrd\n"
  .set message_len, . - message
