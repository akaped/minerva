// The ELK layout engine (elkjs, EPL-2.0) is not bundled. Diagrams that request
// `layout: elk` report an error; Mermaid's default layouts are unaffected.
export default class ELK {
  layout() {
    return Promise.reject(
      new Error("The ELK diagram layout is not available in Minerva"),
    );
  }
}
