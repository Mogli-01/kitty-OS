# Alvos de fuzzing do roadmap

O fuzzing usa dados de teste no host. A entrada dos parsers é tratada
como não confiável, com limites explícitos de tempo e tamanho.

| Alvo planeado | Fase | Estado |
|---|---|---|
| Descodificação do formato do disco encriptado | 16 | Aguarda o formato e o parser |
| Parsers de pacotes de rede | 17 | Aguarda os parsers da stack |
| Manifestos de pacotes `kpkg` | 18 | Aguarda o formato do manifesto e o parser |

Cada alvo deve ligar o parser real ao harness e cobrir também exemplos
válidos. A integração na CI entra no PR que tornar o alvo executável.
