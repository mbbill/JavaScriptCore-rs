- PropertyTable was a fastMalloc-owned object rather than a JSCell.
- Structure held the table through an OwnPtr and copied it through PassOwnPtr paths.
- Unpinned tables lived as long as the Structure even after the table stopped being needed.

## Moves

- 2013-02-26 (f7da71f2) replaced by [[structure-shapes]]: Unpinned Structure property tables were never freed while the Structure was alive even when no longer needed (14 MB waste on Membuster3); making PropertyTable a GC-managed JSCell allows Structure::visitChildren to null out m_propertyTable for unpinned tables so the GC can collect them. (sourced)
