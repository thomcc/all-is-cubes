// Derived from the template
// https://github.com/rustwasm/rust-webpack-template/blob/24f3af83206b52e0241d95ee10cebf930ec8bf08/template/webpack.config.js

const path = require("path");
const CopyPlugin = require("copy-webpack-plugin");
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");

const dist = path.resolve(__dirname, "dist");

module.exports = {
  mode: "production",
  entry: {
    index: "./js/index.js"
  },
  output: {
    path: dist,
    filename: "[name].js"
  },
  devServer: {
    static: {
      directory: dist,
    },
  },
  experiments: {
    syncWebAssembly: true,
  },
  plugins: [
    new CopyPlugin({
      patterns: [
        { from: path.resolve(__dirname, "static") }
      ]
    }),

    new WasmPackPlugin({
      crateDirectory: __dirname,
    }),
  ],
  ignoreWarnings: [
    // Workaround for https://github.com/rust-random/getrandom/issues/224
    (warning) =>
      warning.message ===
      "Critical dependency: the request of a dependency is an expression",
  ]
};
