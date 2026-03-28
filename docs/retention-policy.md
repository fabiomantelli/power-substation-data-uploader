# Política de Retenção Local — Subestação

## Princípio

A subestação não é o repositório definitivo.
O ONS é o arquivo histórico oficial.
A SE mantém retenção operacional para resiliência e reenvio.

## Áreas e Regras

| Área   | Conteúdo               | Regra de Retenção              | Remoção Automática |
|--------|------------------------|--------------------------------|--------------------|
| queue/ | Pendentes de envio     | Nunca por idade                | Não                |
| spool/ | Em processamento       | Limpo após envio ou erro       | Sim (automático)   |
| sent/  | Enviados e confirmados | 30 dias (padrão), 90 (máximo)  | Sim, por política  |
| error/ | Falhas permanentes     | 60 dias                        | Sim                |
| logs/  | Logs do agente         | 30 dias                        | Sim                |

## Retenção Híbrida (Tempo + Capacidade)

A retenção efetiva é determinada pelo menor dos dois limites:

```
retencao_efetiva = min(sent_retention_max_days, limitado_por_disco)
```

### Watermarks do Disco (configurados em agent.toml)

| Nível   | Uso do Disco  | Ação                                              |
|---------|---------------|---------------------------------------------------|
| Aviso   | >= 70%        | Log de alerta                                     |
| Redução | >= 80%        | Reduz retenção para sent_retention_days (mínimo)  |
| Crítico | >= 90%        | Força retenção de 7 dias; limpeza agressiva        |
| Reserva | < min_free_mb | Força limpeza agressiva                           |

### Regras Invioláveis

1. queue/ nunca é limpa por idade
2. Reserva mínima de espaço sempre preservada (min_free_mb)
3. Limpeza automática ocorre apenas em sent/ e error/
4. Eventos em quarentena no ONS não são removidos localmente até investigação

## Dimensionamento Recomendado

```
retencao_segura_dias = espaco_reservado_para_sent / volume_diario_p95 * 0.8
```

Usar sempre o percentil 95 do volume diário, não a média.

## Configuração

Ver `config/agent.example.toml`, seção `[retention]` e `[disk]`.
