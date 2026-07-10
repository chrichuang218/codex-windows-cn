# Detect installed product by package contents

The Microsoft Store product keeps the `OpenAI.Codex` package identity while its public title and executable layout can evolve. The launcher will classify each extracted version by validated package contents: prefer `ChatGPT.exe` when present, otherwise use `Codex.exe`.

This avoids a brittle hardcoded version boundary, launches the correct entrypoint for the observed `26.707.3748.0` transition, and remains compatible with future Store naming changes. Version numbers remain useful for ordering and regression fixtures, but are not the source of truth for product identity.
