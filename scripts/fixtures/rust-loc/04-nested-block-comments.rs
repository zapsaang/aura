fn visible() {}
/* outer
   /* nested { } */
   #[cfg(test)] mod fake { fn hidden() {} }
*/
fn visible_again() {}
