- One offset-extractor generation step handles settings enumeration and structure offset extraction together.
- Desired offset headers embed both settings and offset tables from a combined extraction pass.
- Configuration combinations cannot be generated independently before offset extraction.

## Moves

- 2018-10-16 (08c63ef8) replaced by [[llint]]: Configuration/settings extraction was separated from offset extraction so that the settings binary (LLIntSettingsExtractor) can be built and run before the offset extractor, enabling the assembler to generate correct code for each configuration combination independently. (sourced)
