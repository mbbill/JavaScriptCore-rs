- JITMathIC stores inline size and slow-path locations as int32 byte offsets from the inline start.

## Moves

- 2018-09-27 (fea5bfb0) replaced by [[math-ics]]: int32_t fields (m_inlineSize, m_deltaFromStartToSlowPathCallLocation, m_deltaFromStartToSlowPathStart) cannot carry ARM64E pointer authentication tags, so they were replaced with typed CodeLocation<Tag> smart pointers (m_inlineEnd, m_slowPathCallLocation, m_slowPathStartLocation) that encode the pointer tag in their type parameter. (code)
