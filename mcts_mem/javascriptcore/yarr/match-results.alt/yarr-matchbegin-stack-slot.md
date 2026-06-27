- push(index) to save matchBegin on call stack at disjunction entry
- poke(regT0/index, m_callFrameSize) for disjunction alternatives
- ++frameSize to account for matchBegin stack slot
- pop(returnRegister) then store32(returnRegister,output) at generate() exit

## Moves

- 2010-06-16 (cabdfe60) replaced by [[match-results]]: matchBegin was stored in a dedicated call-stack slot (push/pop at disjunction boundary, +1 frameSize) but the output array passed into the JIT stub already provides a suitable temporary slot at index 0; storing directly there eliminates the extra stack allocation. (sourced)
