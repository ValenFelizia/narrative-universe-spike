# Fixture sintético de I1

Este archivo describe la base que crean `spike seed` y `tests/process_replay.rs`.

El fixture es sintético. No es un registro de canon de War Archivist. G0 pidió un mapeo editorial explícito y una fuente precisa antes de guardar la membresía Narrador a Casa como ACORDADO. Este incremento no tiene ese mapeo. Por eso la afirmación queda en PROVISIONAL, las etiquetas dicen que los registros son sintéticos, y `source_status_raw` queda ausente.

La página no es la página principal de ninguna entidad. Las entidades no tienen página principal. La definición pertenece al tipo Personaje, admite una referencia y declara cardinalidad singular. Hay una sola afirmación. La consulta desde Narrador sintético y la consulta hacia Casa sintética devuelven ese mismo id. No existe una fila inversa.

`scope_note` vale `al inicio`. Es un texto sintético para probar el campo. No afirma un hecho de la biblia.

## Identificadores

| Registro | Id |
| --- | --- |
| Universo | `11111111-1111-4111-8111-111111111111` |
| Página | `22222222-2222-4222-8222-222222222222` |
| Tipo Personaje | `33333333-3333-4333-8333-333333333333` |
| Tipo Casa | `44444444-4444-4444-8444-444444444444` |
| Entidad Narrador sintético | `55555555-5555-4555-8555-555555555555` |
| Entidad Casa sintética | `66666666-6666-4666-8666-666666666666` |
| Definición de pertenencia sintética | `77777777-7777-4777-8777-777777777777` |
| Afirmación | `88888888-8888-4888-8888-888888888888` |

## Texto literal

El título de la página es `Página libre`.

El cuerpo de la página es `Texto con acento áéíóú ñ y emoji 📚`.

Los recuentos esperados son 1 página, 2 tipos, 2 entidades, 1 definición y 1 afirmación.
