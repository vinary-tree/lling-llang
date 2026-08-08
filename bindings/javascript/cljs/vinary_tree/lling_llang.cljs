(ns vinary-tree.lling-llang
  "Idiomatic ClojureScript facade for scalar WFST construction and composition."
  (:require ["@vinary-tree/lling-llang" :as native]))

(defn vector-wfst [] (native/vectorWfst))
(defn add-state! [builder] (.addState builder))
(defn set-start! [builder state] (.setStart builder state))
(defn set-final!
  ([builder state] (set-final! builder state 0))
  ([builder state weight] (.setFinal builder state weight)))
(defn add-arc!
  ([builder from input output to] (add-arc! builder from input output to 0))
  ([builder from input output to weight]
   (.addArc builder from input output to weight)))
(defn build! [builder] (.build builder))
(defn compose [first second] (native/compose first second))
(defn start [wfst] (.start wfst))
(defn state [wfst state-id] (js->clj (.state wfst state-id) :keywordize-keys true))
(defn close! [resource] (.close resource))
