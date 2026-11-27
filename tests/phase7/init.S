.globl _start
.text
_start:
  mov $3, %eax
  xor %edi, %edi
  lea message(%rip), %rsi
  mov $message_len, %edx
  syscall
  xor %edi, %edi
  xor %eax, %eax
  syscall
  ud2
message:
  .ascii "[init-test] static ELF executed from initrd\n"
  .set message_len, . - message
