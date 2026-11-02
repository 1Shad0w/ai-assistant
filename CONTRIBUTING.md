# Guía de contribución

¡Gracias por colaborar! Esta guía resume cómo trabajamos.

## Política de ramas

| Rama | Propósito |
|---|---|
| `main` | Protegida. Solo versiones estables y lanzamientos (etiquetados `vX.Y.Z`). |
| `develop` | Integración continua. Destino de todos los PR de funcionalidades. |
| `feat/NN-nombre` | Una rama por fase o funcionalidad (p. ej. `feat/02-cloud-llm-omniroute`). |
| `fix/descripcion` | Correcciones de errores. |

Flujo: `feat/*` → PR a `develop` (squash o merge con CI en verde) → `develop` → PR a `main` en cada release.
Nunca hagas push directo a `main` ni `develop`. Las ramas de fase se crean desde `develop` y se mantienen al día con `git rebase develop`.

## Entorno

Rust estable (≥ 1.88), CMake y Node.js (ver [README](README.md)).

## Formato (rustfmt)

```bash
cargo fmt --all
cargo fmt --all -- --check   # lo que ejecuta la CI
```

## Validación (clippy y pruebas)

Antes de abrir un PR deben pasar sin advertencias:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Estándares de código

- Sin `unwrap()` ni `expect()` en código de producción; propaga errores con `AssistantError` / `?`.
- Sin `unsafe` salvo justificación explícita en el PR.
- Documenta todo ítem público (`missing_docs`).
- Los contratos compartidos viven en `core-types`; no añadas ahí dependencias de red, audio o UI.
- El backend no puede depender de la UI.

## Política de commits

Usamos [Conventional Commits](https://www.conventionalcommits.org/):

```text
<tipo>(<ámbito>): <resumen en imperativo, máx. 72 caracteres>
```

Tipos: `feat`, `fix`, `docs`, `refactor`, `test`, `perf`, `build`, `ci`, `chore`.
Ejemplo: `feat(llm-client): parsear deltas SSE de tool calls`.

- Commits pequeños y atómicos; cada uno debe compilar.
- Referencia issues en el cuerpo (`Closes #12`).
- Cambios incompatibles: `!` tras el ámbito y sección `BREAKING CHANGE:`.

## Pull requests

1. Describe el qué y el porqué; enlaza la fase correspondiente.
2. Incluye pruebas y actualiza la documentación afectada.
3. Espera al menos una revisión aprobada y la CI en verde.

## Licencia

Al contribuir aceptas que tu aportación se licencia bajo [MIT](LICENSE).

