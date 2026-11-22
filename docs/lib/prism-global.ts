import { Prism } from "prism-react-renderer";

// prismjs language components register themselves on a global `Prism`.
(globalThis as unknown as { Prism: typeof Prism }).Prism = Prism;
