# Bundled browser dependency notices

The browser bundle includes Yjs, lib0, and their browser support dependencies. Their licenses follow. Other Rust/build/test dependencies retain their own package licenses.

## Browser bundles

`web/dist/app.js` bundles Yjs and its support libraries (licenses below), Turndown, and turndown-plugin-gfm. `web/dist/mermaid.js` is loaded only when a note contains a Mermaid diagram. It bundles Mermaid and its dependencies, with license comments extracted to `web/dist/mermaid.js.LEGAL.txt`. The ELK layout engine (elkjs, EPL-2.0) is deliberately excluded and replaced by `web/elk-stub.js`. Every third-party package in the two bundles:

| Package | Version | License |
| --- | --- | --- |
| @braintree/sanitize-url | 7.1.2 | MIT |
| @chevrotain/cst-dts-gen | 13.2.0 | Apache-2.0 |
| @chevrotain/gast | 13.2.0 | Apache-2.0 |
| @chevrotain/regexp-to-ast | 13.2.0 | Apache-2.0 |
| @chevrotain/utils | 13.2.0 | Apache-2.0 |
| @iconify/utils | 3.1.7 | MIT |
| @mermaid-js/parser | 2.0.1 | MIT |
| @upsetjs/venn.js | 2.0.0 | MIT |
| chevrotain | 13.2.0 | Apache-2.0 |
| cose-base | 2.2.0 | MIT |
| cytoscape | 3.34.4 | MIT |
| cytoscape-cose-bilkent | 4.1.0 | MIT |
| cytoscape-fcose | 2.2.0 | MIT |
| d3 | 7.9.0 | ISC |
| d3-array | 2.12.1 | BSD-3-Clause |
| d3-axis | 3.0.0 | ISC |
| d3-brush | 3.0.0 | ISC |
| d3-chord | 3.0.1 | ISC |
| d3-color | 3.1.0 | ISC |
| d3-contour | 4.0.2 | ISC |
| d3-delaunay | 6.0.4 | ISC |
| d3-dispatch | 3.0.1 | ISC |
| d3-drag | 3.0.0 | ISC |
| d3-dsv | 3.0.1 | ISC |
| d3-ease | 3.0.1 | BSD-3-Clause |
| d3-fetch | 3.0.1 | ISC |
| d3-force | 3.0.0 | ISC |
| d3-format | 3.1.2 | ISC |
| d3-geo | 3.1.1 | ISC |
| d3-hierarchy | 3.1.2 | ISC |
| d3-interpolate | 3.0.1 | ISC |
| d3-path | 1.0.9 | BSD-3-Clause |
| d3-polygon | 3.0.1 | ISC |
| d3-quadtree | 3.0.1 | ISC |
| d3-random | 3.0.1 | ISC |
| d3-sankey | 0.12.3 | BSD-3-Clause |
| d3-scale | 4.0.2 | ISC |
| d3-scale-chromatic | 3.1.0 | ISC |
| d3-selection | 3.0.0 | ISC |
| d3-shape | 1.3.7 | BSD-3-Clause |
| d3-time | 3.1.0 | ISC |
| d3-time-format | 4.1.0 | ISC |
| d3-timer | 3.0.1 | ISC |
| d3-transition | 3.0.1 | ISC |
| d3-zoom | 3.0.0 | ISC |
| dagre-d3-es | 7.0.14 | MIT |
| dayjs | 1.11.23 | MIT |
| delaunator | 5.1.0 | ISC |
| dompurify | 3.4.17 | (MPL-2.0 OR Apache-2.0) |
| es-toolkit | 1.53.0 | MIT |
| internmap | 1.0.1 | ISC |
| katex | 0.18.10 | MIT |
| khroma | 2.1.0 | MIT (license file) |
| layout-base | 2.0.1 | MIT |
| lib0 | 0.2.119 | MIT |
| lodash-es | 4.18.1 | MIT |
| marked | 16.4.2 | MIT |
| mermaid | 12.2.0 | MIT |
| robust-predicates | 3.0.3 | Unlicense |
| roughjs | 4.6.6 | MIT |
| stylis | 4.4.0 | MIT |
| ts-dedent | 2.3.0 | MIT |
| turndown | 7.2.4 | MIT |
| turndown-plugin-gfm | 1.0.2 | MIT |
| uuid | 14.0.3 | MIT |
| yjs | 13.6.33 | MIT |

The server renders math with the `math-core` crate (MIT) and highlights code with `syntect` (MIT); Rust dependencies retain their own licenses.

## Desktop client: WebView2Loader.dll

The Windows desktop package redistributes `WebView2Loader.dll` from the Microsoft Edge WebView2 SDK (taken from the `webview2-com-sys` crate). It is © Microsoft Corporation and licensed under the WebView2 SDK license, which is distributed with the `Microsoft.Web.WebView2` NuGet package (`LICENSE.txt`). Include that license text in Windows release archives. The desktop binaries statically link Rust crates (tao, wry, muda, keyring, reqwest, rustls, and their dependencies) under their own MIT/Apache-2.0 or compatible licenses.

## yjs

The MIT License (MIT)

Copyright (c) 2023
  - Kevin Jahns <kevin.jahns@protonmail.com>.
  - Chair of Computer Science 5 (Databases & Information Systems), RWTH Aachen University, Germany

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.


## lib0

The MIT License (MIT)

Copyright (c) 2019 Kevin Jahns <kevin.jahns@protonmail.com>.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.


## isomorphic.js

The MIT License (MIT)

Copyright (c) 2020 Kevin Jahns <kevin.jahns@protonmail.com>.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Lucide (brain icon)

The AI chat's brain icon (`web/app.js`) is drawn from the Lucide icon set, https://lucide.dev, under the ISC License.

ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
