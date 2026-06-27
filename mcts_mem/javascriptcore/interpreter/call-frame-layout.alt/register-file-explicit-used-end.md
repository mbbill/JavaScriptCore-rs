- RegisterFile stores both committed capacity and a separate logical used end.
- Grow and shrink operations update m_end, and root scanning walks begin() through end().
- Reentry decisions depend on the stored logical end rather than deriving extent from topCallFrame.

## Moves

- 2012-04-27 (4bbf6b45) replaced by [[call-frame-layout]]: Register-file users only needed committed capacity plus the active frame extent, so deriving reusable/marked range from topCallFrame makes GC mark only the used portion and prevents VM re-entry from exhausting the register file as quickly. (sourced)
