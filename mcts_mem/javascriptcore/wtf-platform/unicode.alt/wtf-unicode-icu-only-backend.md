- WTF Unicode dispatches only to Qt4 or ICU implementations.
- GTK must depend on ICU for the WTF Unicode layer even when GLib can provide the needed operations.
- Text codecs and text-break behavior are not separable from the basic Unicode helper backend.

## Moves

- 2009-05-22 (cfb136b6) replaced by [[unicode]]: The GTK port needed to reduce the ICU dependency footprint; adding USE(GLIB_UNICODE) as a third dispatch branch in Unicode.h (alongside Qt4 and ICU) allowed the WTF Unicode layer to be implemented on GLib while text codecs and TextBreakIterator remain ICU-based via the hybrid macro. (sourced)
