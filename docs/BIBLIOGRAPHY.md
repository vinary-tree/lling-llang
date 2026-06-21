# Bibliography

The canonical, citation-checked reference list for all `lling-llang` documentation.
Every entry below has been verified; DOIs and arXiv identifiers resolve to the
stated work. Topic docs cite into this file by anchor, e.g.
`[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)` (adjust the `../` depth to the
citing file's directory level — see [`STYLE.md`](STYLE.md)).

When adding a reference: confirm the work exists, prefer a DOI, fall back to an
arXiv / ACL Anthology / PMLR permalink only when no DOI is minted, and **never
fabricate a DOI**.

---

## Weighted automata & parsing foundations

- <a id="ref-mohri2002"></a>**[Mohri 2002]** Mohri, M., Pereira, F., & Riley, M. (2002).
  *Weighted Finite-State Transducers in Speech Recognition.* Computer Speech &
  Language 16(1):69–88.
  [doi:10.1006/csla.2001.0184](https://doi.org/10.1006/csla.2001.0184)
- <a id="ref-mohri1997"></a>**[Mohri 1997]** Mohri, M. (1997).
  *Finite-State Transducers in Language and Speech Processing.* Computational
  Linguistics 23(2):269–311.
  [ACL J97-2003](https://aclanthology.org/J97-2003/)
- <a id="ref-mohri2009"></a>**[Mohri 2009]** Mohri, M. (2009).
  *Weighted Automata Algorithms.* In *Handbook of Weighted Automata*, pp. 213–254.
  Springer.
  [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6)
- <a id="ref-earley1970"></a>**[Earley 1970]** Earley, J. (1970).
  *An Efficient Context-Free Parsing Algorithm.* Communications of the ACM
  13(2):94–102.
  [doi:10.1145/362007.362035](https://doi.org/10.1145/362007.362035)
- <a id="ref-goodman1999"></a>**[Goodman 1999]** Goodman, J. (1999).
  *Semiring Parsing.* Computational Linguistics 25(4):573–605.
  [ACL J99-4004](https://aclanthology.org/J99-4004/)

## Speech & sequence models

- <a id="ref-graves2006"></a>**[Graves 2006]** Graves, A., Fernández, S., Gomez, F.,
  & Schmidhuber, J. (2006). *Connectionist Temporal Classification: Labelling
  Unsegmented Sequence Data with Recurrent Neural Networks.* ICML '06:369–376.
  [doi:10.1145/1143844.1143891](https://doi.org/10.1145/1143844.1143891)
- <a id="ref-graves2012"></a>**[Graves 2012]** Graves, A. (2012).
  *Sequence Transduction with Recurrent Neural Networks.*
  [arXiv:1211.3711](https://arxiv.org/abs/1211.3711)
- <a id="ref-miao2015"></a>**[Miao 2015]** Miao, Y., Gowayyed, M., & Metze, F. (2015).
  *EESEN: End-to-End Speech Recognition using Deep RNN Models and WFST-based
  Decoding.* ASRU 2015.
  [doi:10.1109/ASRU.2015.7404790](https://doi.org/10.1109/ASRU.2015.7404790)
- <a id="ref-laptev2022"></a>**[Laptev 2022]** Laptev, A., Majumdar, S., &
  Ginsburg, B. (2022). *CTC Variations Through New WFST Topologies.* Interspeech
  2022:1041–1045.
  [doi:10.21437/Interspeech.2022-10854](https://doi.org/10.21437/Interspeech.2022-10854)
- <a id="ref-povey2016"></a>**[Povey 2016]** Povey, D., Peddinti, V., Galvez, D.,
  et al. (2016). *Purely Sequence-Trained Neural Networks for ASR Based on
  Lattice-Free MMI.* Interspeech 2016:2751–2755.
  [doi:10.21437/Interspeech.2016-595](https://doi.org/10.21437/Interspeech.2016-595)
- <a id="ref-tian2022"></a>**[Tian 2022]** Tian, J., Yu, J., Weng, C., Zou, Y., &
  Yu, D. (2022). *Integrating Lattice-Free MMI into End-to-End Speech Recognition.*
  [arXiv:2203.15614](https://arxiv.org/abs/2203.15614)
- <a id="ref-gao2025"></a>**[Gao 2025]** Gao, D., Liao, C., Liu, C., et al. (2025).
  *WST: Weakly Supervised Transducer for Automatic Speech Recognition.*
  [arXiv:2511.04035](https://arxiv.org/abs/2511.04035)

## Differentiable & GPU decoding

- <a id="ref-hannun2020"></a>**[Hannun 2020]** Hannun, A., Pratap, V., Kahn, J., &
  Hsu, W.-N. (2020). *Differentiable Weighted Finite-State Transducers.* ICML 2020
  (PMLR 119). [arXiv:2010.01003](https://arxiv.org/abs/2010.01003)
  · *(Note: published at ICML 2020 — earlier drafts of some docs miscited this as
  "ICLR 2021"; the correct venue is ICML 2020.)*
- <a id="ref-braun2020"></a>**[Braun 2020]** Braun, H., Luitjens, J., Leary, R.,
  Kaldewey, T., & Galvez, D. (2020). *GPU-Accelerated Viterbi Exact Lattice Decoder
  for Batched Online and Offline Speech Recognition.* ICASSP 2020:7874–7878.
  [doi:10.1109/ICASSP40776.2020.9054099](https://doi.org/10.1109/ICASSP40776.2020.9054099)

## Online learning & text normalization

- <a id="ref-cortes2015"></a>**[Cortes 2015]** Cortes, C., Kuznetsov, V., Mohri, M.,
  & Warmuth, M. K. (2015). *On-Line Learning Algorithms for Path Experts with
  Non-Additive Losses.* COLT 2015, PMLR 40:424–447.
  [proceedings.mlr.press/v40/Cortes15](https://proceedings.mlr.press/v40/Cortes15.html)
- <a id="ref-zhang2021"></a>**[Zhang 2021]** Zhang, Y., Bakhturina, E., Gorman, K.,
  & Ginsburg, B. (2021). *NeMo Inverse Text Normalization: From Development to
  Production.* [arXiv:2104.05055](https://arxiv.org/abs/2104.05055)

---

## Additional references cited by individual modules

These support specific module docs; each is verified before inclusion.

- <a id="ref-park2025"></a>**[Park 2025]** Park, K., Zhou, T., & D'Antoni, L. (2025).
  *Flexible and Efficient Grammar-Constrained Decoding.*
  [arXiv:2502.05111](https://arxiv.org/abs/2502.05111) — cited by
  [`docs/advanced/constrained-decoding.md`](advanced/constrained-decoding.md).
- <a id="ref-mohri2000"></a>**[Mohri 2000]** Mohri, M. (2000).
  *Minimization Algorithms for Sequential Transducers.* Theoretical Computer
  Science 234(1–2):177–201.
  [doi:10.1016/S0304-3975(98)00115-7](https://doi.org/10.1016/S0304-3975(98)00115-7)
  — cited by [`docs/advanced/subsequential-transducers.md`](advanced/subsequential-transducers.md).
- <a id="ref-fulop2009"></a>**[Fülöp & Vogler 2009]** Fülöp, Z., & Vogler, H. (2009).
  *Weighted Tree Automata and Tree Transducers.* In *Handbook of Weighted Automata*,
  pp. 313–403. Springer.
  [doi:10.1007/978-3-642-01492-5_9](https://doi.org/10.1007/978-3-642-01492-5_9)
  — cited by [`docs/transducers/tree-transducers.md`](transducers/tree-transducers.md).
- <a id="ref-allauzen2007"></a>**[Allauzen 2007]** Allauzen, C., Riley, M.,
  Schalkwyk, J., Skut, W., & Mohri, M. (2007). *OpenFst: A General and Efficient
  Weighted Finite-State Transducer Library.* CIAA 2007, LNCS 4783:11–23.
  [doi:10.1007/978-3-540-76336-9_3](https://doi.org/10.1007/978-3-540-76336-9_3)
  — cited by composition / determinization docs.
- <a id="ref-lv2021"></a>**[Lv 2021]** Lv, H., Povey, D., Yarmohammadi, M., Li, K.,
  Wang, Y., Xie, L., & Khudanpur, S. (2021). *LET-Decoder: A WFST-Based
  Lazy-Evaluation Token-Group Decoder with Exact Lattice Generation.* IEEE Signal
  Processing Letters 28:703–707.
  [doi:10.1109/LSP.2021.3067220](https://doi.org/10.1109/LSP.2021.3067220)
  — cited by [`docs/optimization/token-grouping.md`](optimization/token-grouping.md)
  and [`docs/advanced/beam-optimization.md`](advanced/beam-optimization.md).
- <a id="ref-kahn1962"></a>**[Kahn 1962]** Kahn, A. B. (1962). *Topological Sorting
  of Large Networks.* Communications of the ACM 5(11):558–562.
  [doi:10.1145/368996.369025](https://doi.org/10.1145/368996.369025)
  — cited by [`docs/algorithms/topological-sort.md`](algorithms/topological-sort.md).
- <a id="ref-huang2005"></a>**[Huang & Chiang 2005]** Huang, L., & Chiang, D. (2005).
  *Better k-best Parsing.* IWPT 2005:53–64.
  [ACL W05-1506](https://aclanthology.org/W05-1506/)
  — cited by [`docs/algorithms/path-extraction.md`](algorithms/path-extraction.md).
