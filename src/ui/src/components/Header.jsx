import React from 'react'
import 'style/header.scss'

function Header (props) {
  return (
    <header>
      <div className='title'>
        <h1> {props.title} </h1>
      </div>
    </header>
  )
}

export default Header
