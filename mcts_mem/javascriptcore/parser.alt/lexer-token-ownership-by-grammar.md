- Grammar actions owned identifier and string token payloads and deleted them in individual production reductions.
- Parse error recovery could bypass those production-local cleanup actions.

## Moves

- 2003-10-30 (b88c4f2b) replaced by [[parser]]: Grammar action ownership (delete yyvsp[0].ustr / delete yyvsp[0].ident in each production) leaked on parse error paths because error recovery skipped cleanup; moving ownership to the lexer via doneParsing() ensures cleanup on all exit paths including errors. (sourced)
