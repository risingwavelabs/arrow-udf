// Exposes the bundled big.js (see `big.js` and `LICENCE.md`) as the global `BigDecimal`,
// the JavaScript representation of Arrow decimal values.

(function () {
  'use strict';

  var Big = globalThis.Big;
  delete globalThis.Big;

  // Never format in exponent notation, whatever the magnitude.
  Big.NE = -1e6;
  Big.PE = 1e6;

  var P = Big.prototype;

  P.add = P.plus;
  P.sub = P.minus;
  P.mul = P.times;

  // Operators on decimals would silently degrade to string concatenation or reference
  // comparison, so reject them instead of returning a wrong answer. `toString` is left
  // alone, so string conversion and template literals keep working.
  P.valueOf = function () {
    throw new TypeError(
      'BigDecimal does not support arithmetic operators, ' +
      'use methods like add()/sub()/mul()/div()/cmp() instead'
    );
  };

  function BigDecimal(v) {
    return Big(v);
  }

  BigDecimal.prototype = P;
  P.constructor = BigDecimal;

  globalThis.BigDecimal = BigDecimal;
})();
