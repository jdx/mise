"use strict";

module.exports = (ajv) => {
  for (const format of ["date", "date-time-local", "date-time", "time-local"]) {
    ajv.addFormat(format, true);
  }
};
