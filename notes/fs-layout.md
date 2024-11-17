# Store

* pkg
  * by-name
    * _name_
      * _hash_ > pkg/by-hash/_hash_
  * by-hash
    * _hash_
      * src
        * ...
      * _target_
        * ...
* link
  * _lock hash_
    * _name_-_hash_-_target_ > pkg/by-hash/_hash_/_target_
* root
  * _lock hash_ > link/_lock hash_

# Inside a build

Designed so that relative rpaths work.

* build
  * src
  * for each dep: _name_-_hash_-_target_ > pkg/by-hash/_hash_/_target_
  * for each out: _name_-_hash_-_target_
