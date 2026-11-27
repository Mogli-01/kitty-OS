# Testes de regressão de segurança

O workflow executa os testes defensivos registados no manifesto. Cada
caso tem um identificador estável e uma verificação de uma proteção
esperada em QEMU. O runner valida os marcadores da série e o estado de
saída do teste. Um erro de compilação, um timeout ou um marcador ausente
faz falhar o job. A CI arranca o convidado com a rede desativada.

- `checks` lista os identificadores implementados no runner.
- `open_issues` regista exceções para acompanhamento. Cada entrada tem
  `check` e `issue` (número da issue). Os checks continuam obrigatórios.

Para repetir em QEMU:

```sh
python3 tests/security/redteam/run.py
```

O formato da ronda e a coordenação de falhas estão em
[REDTEAM.md](../../../docs/REDTEAM.md).
